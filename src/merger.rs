use crate::classifier::ClassifiedEvent;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

/// A merged, ready-to-push time block representing contiguous work on one project.
#[derive(Debug, Clone)]
pub struct TimeBlock {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub project_id: u32,
    pub activity_id: u32,
    /// Human-readable description for the Kimai timesheet entry. Built from
    /// every event that contributed to this block, not just the first one.
    pub description: String,
    /// Number of raw AW events that were merged into this block
    #[allow(dead_code)] // kept for tests + future telemetry
    pub event_count: usize,
}

impl TimeBlock {
    pub fn duration_secs(&self) -> i64 {
        (self.end - self.start).num_seconds()
    }
}

/// In-progress block. We accumulate per-event detail and finalise the
/// description string only when the block is flushed.
struct BlockBuilder {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    project_id: u32,
    activity_id: u32,
    event_count: usize,
    /// Key: (title, app, url). Value: aggregated seconds for that combo.
    /// BTreeMap because we want stable iteration order for tests and logs.
    details: BTreeMap<DetailKey, i64>,
}

impl BlockBuilder {
    fn from_event(ev: &ClassifiedEvent) -> Self {
        let mut b = Self {
            start: ev.event.timestamp,
            end: ev.event.end(),
            project_id: ev.project_id,
            activity_id: ev.activity_id,
            event_count: 0,
            details: BTreeMap::new(),
        };
        b.add_event(ev);
        b
    }

    fn add_event(&mut self, ev: &ClassifiedEvent) {
        let ev_end = ev.event.end();
        if ev_end > self.end {
            self.end = ev_end;
        }
        self.event_count += 1;
        let key = (
            ev.event.data.title.clone(),
            ev.event.data.app.clone(),
            ev.event.data.url.clone(),
        );
        let secs = ev.event.duration.max(0.0) as i64;
        *self.details.entry(key).or_insert(0) += secs;
    }

    fn duration_secs(&self) -> i64 {
        (self.end - self.start).num_seconds()
    }

    fn finish(self) -> TimeBlock {
        let description = build_description(&self.details);
        TimeBlock {
            start: self.start,
            end: self.end,
            project_id: self.project_id,
            activity_id: self.activity_id,
            description,
            event_count: self.event_count,
        }
    }
}

/// Maximum distinct title/app/url combos shown in the description before we
/// collapse the rest into a "(+N more)" tail. Tuned to fit the most useful
/// detail in well under Kimai's 255-char limit.
const MAX_DETAILS_SHOWN: usize = 5;

/// Key used to dedupe events that contributed to a block: (title, app, url).
type DetailKey = (String, String, Option<String>);

fn build_description(details: &BTreeMap<DetailKey, i64>) -> String {
    if details.is_empty() {
        return String::new();
    }

    // Sort by contributed seconds desc so the most-meaningful entries appear first
    let mut sorted: Vec<(&DetailKey, &i64)> = details.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));

    let total = sorted.len();
    let shown = sorted.iter().take(MAX_DETAILS_SHOWN);

    let mut parts: Vec<String> = shown
        .map(|((title, app, url), _)| format_one(title, app, url.as_deref()))
        .collect();

    if total > MAX_DETAILS_SHOWN {
        parts.push(format!("(+{} more)", total - MAX_DETAILS_SHOWN));
    }

    parts.join(" | ")
}

fn format_one(title: &str, app: &str, url: Option<&str>) -> String {
    let title = title.trim();
    let app = app.trim();

    let head = if title.is_empty() {
        app.to_string()
    } else if app.is_empty() {
        title.to_string()
    } else {
        format!("{title} [{app}]")
    };

    match url {
        Some(u) if !u.is_empty() => format!("{head} <{u}>"),
        _ => head,
    }
}

pub struct Merger {
    /// Gaps shorter than this (seconds) between same-project events are bridged
    idle_threshold_secs: i64,
    /// Blocks shorter than this (seconds) after merging are discarded
    min_duration_secs: i64,
}

impl Merger {
    pub fn new(idle_threshold_secs: i64, min_duration_secs: i64) -> Self {
        Self {
            idle_threshold_secs,
            min_duration_secs,
        }
    }

    /// Takes a list of classified events (assumed to be sorted ascending by
    /// timestamp) and returns deduplicated, merged TimeBlocks.
    pub fn merge(&self, mut events: Vec<ClassifiedEvent>) -> Vec<TimeBlock> {
        if events.is_empty() {
            return vec![];
        }

        // Sort by timestamp ascending (AW usually returns them this way but
        // don't rely on it)
        events.sort_by_key(|e| e.event.timestamp);

        let mut blocks: Vec<TimeBlock> = Vec::new();
        let mut current = BlockBuilder::from_event(&events[0]);

        for ev in events.iter().skip(1) {
            let gap_secs = (ev.event.timestamp - current.end).num_seconds();

            let same_project =
                ev.project_id == current.project_id && ev.activity_id == current.activity_id;

            if same_project && gap_secs <= self.idle_threshold_secs {
                current.add_event(ev);
            } else {
                // Flush current block and start a new one
                if current.duration_secs() >= self.min_duration_secs {
                    blocks.push(current.finish());
                } else {
                    tracing::debug!(
                        project = current.project_id,
                        duration = current.duration_secs(),
                        "Dropping short block"
                    );
                }
                current = BlockBuilder::from_event(ev);
            }
        }

        // Don't forget the last open block
        if current.duration_secs() >= self.min_duration_secs {
            blocks.push(current.finish());
        }

        tracing::debug!(
            input_events = events.len(),
            output_blocks = blocks.len(),
            "Merge complete"
        );

        blocks
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aw_client::{AwEvent, AwEventData};
    use chrono::{Duration, Utc};

    fn make_classified(
        start_offset_secs: i64,
        duration_secs: f64,
        project_id: u32,
    ) -> ClassifiedEvent {
        make_classified_with(start_offset_secs, duration_secs, project_id, "test-title", "test-app", None)
    }

    fn make_classified_with(
        start_offset_secs: i64,
        duration_secs: f64,
        project_id: u32,
        title: &str,
        app: &str,
        url: Option<&str>,
    ) -> ClassifiedEvent {
        let base = Utc::now();
        ClassifiedEvent {
            event: AwEvent {
                id: None,
                timestamp: base + Duration::seconds(start_offset_secs),
                duration: duration_secs,
                data: AwEventData {
                    title: title.to_string(),
                    app: app.to_string(),
                    url: url.map(|s| s.to_string()),
                },
            },
            project_id,
            activity_id: 1,
            label: "test".to_string(),
        }
    }

    #[test]
    fn merges_adjacent_same_project() {
        let merger = Merger::new(120, 60);
        let events = vec![
            make_classified(0, 120.0, 10),
            make_classified(150, 120.0, 10),
        ];
        let blocks = merger.merge(events);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].event_count, 2);
        assert_eq!(blocks[0].project_id, 10);
    }

    #[test]
    fn splits_across_project_boundary() {
        let merger = Merger::new(120, 60);
        let events = vec![
            make_classified(0, 120.0, 10),
            make_classified(130, 120.0, 20),
        ];
        let blocks = merger.merge(events);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].project_id, 10);
        assert_eq!(blocks[1].project_id, 20);
    }

    #[test]
    fn drops_short_blocks() {
        let merger = Merger::new(120, 60);
        let events = vec![make_classified(0, 30.0, 10)];
        let blocks = merger.merge(events);
        assert!(blocks.is_empty());
    }

    #[test]
    fn splits_on_large_gap() {
        let merger = Merger::new(120, 60);
        let events = vec![
            make_classified(0, 120.0, 10),
            make_classified(420, 120.0, 10),
        ];
        let blocks = merger.merge(events);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn description_includes_app_and_url() {
        let merger = Merger::new(120, 60);
        let events = vec![
            make_classified_with(0, 120.0, 10, "PR #1", "Firefox", Some("https://example.com/1")),
        ];
        let blocks = merger.merge(events);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].description.contains("PR #1"));
        assert!(blocks[0].description.contains("Firefox"));
        assert!(blocks[0].description.contains("example.com/1"));
    }

    #[test]
    fn description_merges_distinct_titles() {
        let merger = Merger::new(120, 60);
        let events = vec![
            make_classified_with(0, 60.0, 10, "Title A", "Editor", None),
            make_classified_with(70, 60.0, 10, "Title B", "Editor", None),
            make_classified_with(140, 60.0, 10, "Title A", "Editor", None),
        ];
        let blocks = merger.merge(events);
        assert_eq!(blocks.len(), 1);
        // Both unique titles appear
        assert!(blocks[0].description.contains("Title A"));
        assert!(blocks[0].description.contains("Title B"));
    }

    #[test]
    fn description_collapses_overflow() {
        let merger = Merger::new(600, 1);
        // 7 distinct titles, separator at index `i*30`
        let events: Vec<_> = (0..7)
            .map(|i| make_classified_with(i * 30, 25.0, 10, &format!("Title {i}"), "App", None))
            .collect();
        let blocks = merger.merge(events);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].description.contains("(+2 more)"),
            "got: {}", blocks[0].description);
    }
}
