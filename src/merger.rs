use crate::classifier::ClassifiedEvent;
use chrono::{DateTime, Utc};

/// A merged, ready-to-push time block representing contiguous work on one project.
#[derive(Debug, Clone)]
pub struct TimeBlock {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub project_id: u32,
    pub activity_id: u32,
    /// Representative window title for the Kimai description field
    pub description: String,
    /// Number of raw AW events that were merged into this block
    pub event_count: usize,
}

impl TimeBlock {
    pub fn duration_secs(&self) -> i64 {
        (self.end - self.start).num_seconds()
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
        let mut current = self.event_to_block(&events[0]);

        for ev in events.iter().skip(1) {
            let gap_secs = (ev.event.timestamp - current.end).num_seconds();

            let same_project =
                ev.project_id == current.project_id && ev.activity_id == current.activity_id;

            if same_project && gap_secs <= self.idle_threshold_secs {
                // Extend the current block
                let ev_end = ev.event.end();
                if ev_end > current.end {
                    current.end = ev_end;
                }
                current.event_count += 1;
            } else {
                // Flush current block and start a new one
                if current.duration_secs() >= self.min_duration_secs {
                    blocks.push(current);
                } else {
                    tracing::debug!(
                        project = blocks.last().map_or(0, |b| b.project_id),
                        duration = current.duration_secs(),
                        "Dropping short block"
                    );
                }
                current = self.event_to_block(ev);
            }
        }

        // Don't forget the last open block
        if current.duration_secs() >= self.min_duration_secs {
            blocks.push(current);
        }

        tracing::debug!(
            input_events = events.len(),
            output_blocks = blocks.len(),
            "Merge complete"
        );

        blocks
    }

    fn event_to_block(&self, ev: &ClassifiedEvent) -> TimeBlock {
        TimeBlock {
            start: ev.event.timestamp,
            end: ev.event.end(),
            project_id: ev.project_id,
            activity_id: ev.activity_id,
            description: ev.event.data.title.clone(),
            event_count: 1,
        }
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
        let base = Utc::now();
        ClassifiedEvent {
            event: AwEvent {
                id: None,
                timestamp: base + Duration::seconds(start_offset_secs),
                duration: duration_secs,
                data: AwEventData {
                    title: format!("Project {project_id}"),
                    app: "app".to_string(),
                    url: None,
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
        // Two events for project 10, 30s gap (under threshold)
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
            make_classified(130, 120.0, 20), // different project
        ];
        let blocks = merger.merge(events);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].project_id, 10);
        assert_eq!(blocks[1].project_id, 20);
    }

    #[test]
    fn drops_short_blocks() {
        let merger = Merger::new(120, 60);
        // 30s duration — below min_duration_secs of 60
        let events = vec![make_classified(0, 30.0, 10)];
        let blocks = merger.merge(events);
        assert!(blocks.is_empty());
    }

    #[test]
    fn splits_on_large_gap() {
        let merger = Merger::new(120, 60);
        // Same project but 300s gap (above threshold)
        let events = vec![
            make_classified(0, 120.0, 10),
            make_classified(420, 120.0, 10),
        ];
        let blocks = merger.merge(events);
        assert_eq!(blocks.len(), 2, "Large gap should split even same project");
    }
}
