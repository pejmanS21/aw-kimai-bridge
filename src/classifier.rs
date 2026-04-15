use crate::aw_client::AwEvent;
use crate::config::{Config, Rule};
use regex::{Regex, RegexBuilder};

/// A compiled rule ready for matching — holds the pre-built Regex alongside
/// the original config values so we only pay the compile cost once at startup.
struct CompiledRule {
    pattern: Regex,
    project_id: u32,
    activity_id: u32,
    label: String,
}

/// An ActivityWatch event enriched with the Kimai project it was classified to.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClassifiedEvent {
    pub event: AwEvent,
    pub project_id: u32,
    pub activity_id: u32,
    /// Human-readable label of the matched rule (or "default")
    pub label: String,
}

pub struct Classifier {
    rules: Vec<CompiledRule>,
    default_project_id: u32,
    default_activity_id: u32,
}

impl Classifier {
    /// Build a Classifier from the loaded config.
    /// Panics if any regex is invalid — `Config::validate()` should have
    /// caught bad patterns at startup, so this is truly unexpected.
    pub fn from_config(config: &Config) -> Self {
        let rules = config
            .rules
            .iter()
            .map(compile_rule)
            .collect::<Vec<_>>();

        Self {
            rules,
            default_project_id: config.kimai.default_project_id,
            default_activity_id: config.kimai.default_activity_id,
        }
    }

    /// Classify a single event. Returns None if the event should be dropped
    /// (e.g. it matches a rule with project_id = 0, used as an explicit
    /// "ignore" sentinel).
    pub fn classify(&self, event: &AwEvent) -> Option<ClassifiedEvent> {
        let haystack = build_haystack(event);

        for rule in &self.rules {
            if rule.pattern.is_match(&haystack) {
                // project_id = 0 means "explicitly ignore this event"
                if rule.project_id == 0 {
                    tracing::debug!(
                        title = event.data.title,
                        rule = rule.label,
                        "Event ignored by rule"
                    );
                    return None;
                }

                tracing::trace!(
                    title = event.data.title,
                    rule = rule.label,
                    project = rule.project_id,
                    "Event classified"
                );

                return Some(ClassifiedEvent {
                    event: event.clone(),
                    project_id: rule.project_id,
                    activity_id: rule.activity_id,
                    label: rule.label.clone(),
                });
            }
        }

        // No rule matched — fall through to defaults
        tracing::trace!(
            title = event.data.title,
            "No rule matched, using defaults"
        );

        Some(ClassifiedEvent {
            event: event.clone(),
            project_id: self.default_project_id,
            activity_id: self.default_activity_id,
            label: "default".to_string(),
        })
    }

    /// Classify a batch of events, discarding those explicitly ignored.
    pub fn classify_all(&self, events: &[AwEvent]) -> Vec<ClassifiedEvent> {
        events
            .iter()
            .filter_map(|e| self.classify(e))
            .collect()
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build the string we run the regex against.
/// We combine the window title, app name, and URL (if present) separated by
/// newlines so a single pattern can match any field without separate passes.
fn build_haystack(event: &AwEvent) -> String {
    let mut parts = vec![
        event.data.title.as_str(),
        event.data.app.as_str(),
    ];
    if let Some(url) = &event.data.url {
        parts.push(url.as_str());
    }
    parts.join("\n")
}

fn compile_rule(rule: &Rule) -> CompiledRule {
    let pattern = RegexBuilder::new(&rule.pattern)
        .case_insensitive(true)
        .build()
        .unwrap_or_else(|e| panic!("Invalid regex {:?}: {e}", rule.pattern));

    CompiledRule {
        pattern,
        project_id: rule.project_id,
        activity_id: rule.activity_id,
        label: rule
            .label
            .clone()
            .unwrap_or_else(|| rule.pattern.clone()),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aw_client::{AwEvent, AwEventData};
    use chrono::Utc;

    fn make_event(title: &str, app: &str, url: Option<&str>) -> AwEvent {
        AwEvent {
            id: None,
            timestamp: Utc::now(),
            duration: 120.0,
            data: AwEventData {
                title: title.to_string(),
                app: app.to_string(),
                url: url.map(|s| s.to_string()),
            },
        }
    }

    fn make_classifier(rules: Vec<(&str, u32, u32)>) -> Classifier {
        let compiled = rules
            .into_iter()
            .map(|(pat, proj, act)| CompiledRule {
                pattern: RegexBuilder::new(pat)
                    .case_insensitive(true)
                    .build()
                    .unwrap(),
                project_id: proj,
                activity_id: act,
                label: pat.to_string(),
            })
            .collect();

        Classifier {
            rules: compiled,
            default_project_id: 99,
            default_activity_id: 1,
        }
    }

    #[test]
    fn matches_window_title() {
        let c = make_classifier(vec![("github\\.com/myorg", 10, 2)]);
        let ev = make_event("myorg/project-alpha - GitHub", "Firefox", None);
        let result = c.classify(&ev).unwrap();
        assert_eq!(result.project_id, 10);
    }

    #[test]
    fn matches_url() {
        let c = make_classifier(vec![("github\\.com/myorg/alpha", 10, 2)]);
        let ev = make_event("some title", "Chrome", Some("https://github.com/myorg/alpha/pull/12"));
        let result = c.classify(&ev).unwrap();
        assert_eq!(result.project_id, 10);
    }

    #[test]
    fn ignore_rule_returns_none() {
        let c = make_classifier(vec![("YouTube", 0, 0)]);
        let ev = make_event("YouTube - Firefox", "Firefox", None);
        assert!(c.classify(&ev).is_none());
    }

    #[test]
    fn falls_through_to_default() {
        let c = make_classifier(vec![("github\\.com", 10, 2)]);
        let ev = make_event("Random window title", "SomeApp", None);
        let result = c.classify(&ev).unwrap();
        assert_eq!(result.project_id, 99); // default
    }

    #[test]
    fn first_rule_wins() {
        let c = make_classifier(vec![
            ("project-alpha", 10, 2),
            ("project",       20, 3), // broader — should not match
        ]);
        let ev = make_event("project-alpha - VS Code", "code", None);
        let result = c.classify(&ev).unwrap();
        assert_eq!(result.project_id, 10);
    }
}
