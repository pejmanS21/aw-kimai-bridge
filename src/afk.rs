//! AFK overlap handling.
//!
//! ActivityWatch's window watcher keeps emitting events for whatever window
//! has focus, even if the user has stepped away from the machine. To avoid
//! billing time the user wasn't actually working, we subtract the intervals
//! reported by the AFK watcher from each window event.

use crate::aw_client::{AfkEvent, AwEvent};
use chrono::{DateTime, Utc};

/// A closed AFK interval `[start, end)`.
pub type AfkInterval = (DateTime<Utc>, DateTime<Utc>);

/// Convert raw AFK events into the list of intervals we want to subtract.
/// We only care about events whose `data.status == "afk"`.
pub fn intervals_from_events(events: &[AfkEvent]) -> Vec<AfkInterval> {
    let mut intervals: Vec<AfkInterval> = events
        .iter()
        .filter(|e| e.is_afk())
        .map(|e| (e.timestamp, e.end()))
        .filter(|(s, e)| e > s)
        .collect();

    // Sort + merge touching/overlapping intervals so downstream clipping is O(n).
    intervals.sort_by_key(|(s, _)| *s);
    let mut merged: Vec<AfkInterval> = Vec::with_capacity(intervals.len());
    for (s, e) in intervals {
        match merged.last_mut() {
            Some(last) if last.1 >= s => {
                if e > last.1 {
                    last.1 = e;
                }
            }
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// Sub-second slivers left over after clipping aren't worth carrying through
/// the pipeline — they bloat logs and never survive the merger's
/// `min_duration_secs` floor anyway.
const MIN_FRAGMENT_SECS: f64 = 1.0;

/// Subtract every AFK interval from `events`, returning the remaining active
/// fragments. An event that's fully covered by AFK is dropped; one that's
/// partially covered is split into the surviving non-AFK pieces.
///
/// `afk_intervals` must be sorted ascending and non-overlapping. Use
/// [`intervals_from_events`] to produce them.
pub fn clip_events(events: Vec<AwEvent>, afk_intervals: &[AfkInterval]) -> Vec<AwEvent> {
    if afk_intervals.is_empty() {
        return events;
    }

    let mut out = Vec::with_capacity(events.len());
    let mut dropped = 0usize;
    let mut clipped = 0usize;

    for ev in events {
        let ev_start = ev.timestamp;
        let ev_end = ev.end();

        // Find the AFK intervals that touch this event.
        let touching: Vec<AfkInterval> = afk_intervals
            .iter()
            .filter_map(|&(s, e)| {
                let cs = s.max(ev_start);
                let ce = e.min(ev_end);
                if ce > cs { Some((cs, ce)) } else { None }
            })
            .collect();

        if touching.is_empty() {
            out.push(ev);
            continue;
        }

        // Walk left-to-right, emitting fragments between AFK windows.
        let original_fragments = out.len();
        let mut cursor = ev_start;
        for (a_start, a_end) in &touching {
            if *a_start > cursor {
                push_fragment(&mut out, &ev, cursor, *a_start);
            }
            if *a_end > cursor {
                cursor = *a_end;
            }
        }
        if ev_end > cursor {
            push_fragment(&mut out, &ev, cursor, ev_end);
        }

        if out.len() == original_fragments {
            dropped += 1;
        } else {
            clipped += 1;
        }
    }

    if dropped > 0 || clipped > 0 {
        tracing::debug!(
            clipped,
            dropped,
            "Filtered events against AFK intervals"
        );
    }

    out
}

fn push_fragment(out: &mut Vec<AwEvent>, src: &AwEvent, start: DateTime<Utc>, end: DateTime<Utc>) {
    let secs = (end - start).num_milliseconds() as f64 / 1000.0;
    if secs < MIN_FRAGMENT_SECS {
        return;
    }
    out.push(AwEvent {
        id: src.id,
        timestamp: start,
        duration: secs,
        data: src.data.clone(),
    });
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aw_client::{AfkEvent, AfkEventData, AwEvent, AwEventData};
    use chrono::{Duration, TimeZone};

    fn base() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap()
    }

    fn win(offset_secs: i64, duration_secs: f64) -> AwEvent {
        AwEvent {
            id: None,
            timestamp: base() + Duration::seconds(offset_secs),
            duration: duration_secs,
            data: AwEventData {
                title: "T".into(),
                app: "A".into(),
                url: None,
            },
        }
    }

    fn afk(offset_secs: i64, duration_secs: f64, status: &str) -> AfkEvent {
        AfkEvent {
            timestamp: base() + Duration::seconds(offset_secs),
            duration: duration_secs,
            data: AfkEventData { status: status.into() },
        }
    }

    #[test]
    fn no_afk_returns_input() {
        let events = vec![win(0, 300.0)];
        let out = clip_events(events.clone(), &[]);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn drops_event_fully_covered_by_afk() {
        let events = vec![win(60, 60.0)]; // 60..120
        let afks = intervals_from_events(&[afk(0, 300.0, "afk")]);
        let out = clip_events(events, &afks);
        assert!(out.is_empty(), "fully-AFK event must be dropped");
    }

    #[test]
    fn keeps_event_outside_afk() {
        let events = vec![win(400, 60.0)]; // after AFK ends
        let afks = intervals_from_events(&[afk(0, 300.0, "afk")]);
        let out = clip_events(events, &afks);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].duration_secs(), 60);
    }

    #[test]
    fn clips_event_overlapping_afk_start() {
        // event: 100..200, AFK: 150..400
        let events = vec![win(100, 100.0)];
        let afks = intervals_from_events(&[afk(150, 250.0, "afk")]);
        let out = clip_events(events, &afks);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].timestamp, base() + Duration::seconds(100));
        assert_eq!(out[0].duration_secs(), 50);
    }

    #[test]
    fn splits_event_with_afk_in_middle() {
        // event: 0..600, AFK: 200..400 -> two fragments 0..200, 400..600
        let events = vec![win(0, 600.0)];
        let afks = intervals_from_events(&[afk(200, 200.0, "afk")]);
        let out = clip_events(events, &afks);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].duration_secs(), 200);
        assert_eq!(out[1].duration_secs(), 200);
        assert_eq!(out[1].timestamp, base() + Duration::seconds(400));
    }

    #[test]
    fn ignores_not_afk_events() {
        let afks = intervals_from_events(&[afk(0, 1000.0, "not-afk")]);
        let events = vec![win(100, 100.0)];
        let out = clip_events(events, &afks);
        assert_eq!(out.len(), 1, "not-afk events must not clip anything");
    }

    #[test]
    fn merges_overlapping_intervals() {
        let merged = intervals_from_events(&[
            afk(0, 100.0, "afk"),
            afk(50, 100.0, "afk"),  // overlaps previous
            afk(200, 50.0, "afk"),
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].1, base() + Duration::seconds(150));
    }
}
