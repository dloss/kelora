//! Row formatting for `--span-summary`.
//!
//! One record per closed span, carrying the span label, the number of events
//! that survived filters and were included, and every per-window metric that
//! `span.metrics` could express (see `compute_span_metrics`). Rows are data on
//! stdout, so nothing here consults the script-output or metrics suppression
//! flags — only `--silent`, which hushes stdout wholesale, removes them.

use chrono::{DateTime, SecondsFormat, Utc};
use rhai::{Dynamic, Map};

use crate::cli::SpanSummaryFormat;

/// Column name for the built-in event count.
///
/// A user metric may legitimately be called this too (`track_sum("events", 1)`).
/// The built-in keeps the name; `json` separates the two structurally (top-level
/// `events` vs `metrics.events`), while `text` and `tsv` would show two entries
/// with the same key, so the collision is reported once by the caller.
pub const EVENTS_KEY: &str = "events";

/// The label a row is keyed by: the window start for time and idle spans, where
/// it doubles as a plottable x-axis, and the span id otherwise (`#0` for count
/// spans, the field value for field spans).
pub fn span_label(span_id: &str, span_start: Option<DateTime<Utc>>) -> String {
    match span_start {
        Some(start) => start.to_rfc3339_opts(SecondsFormat::Secs, true),
        None => span_id.to_string(),
    }
}

/// Whether a metric map shadows the built-in `events` column in a flat format.
pub fn shadows_events_column(metrics: &Map) -> bool {
    metrics.contains_key(EVENTS_KEY)
}

/// Flatten tabs and newlines out of a cell.
///
/// Field span labels and `track_freq` keys are taken from log data, which can
/// carry both. Left alone, a tab shifts every `tsv` column and a newline splits
/// one span across two records — silently, and in a way that looks like real
/// data downstream. Shared with the cumulative metrics tsv so the two record
/// streams sanitize identically.
fn cell(value: &str) -> String {
    crate::rhai_functions::tracking::tsv_sanitize(value)
}

/// Render a `Dynamic` metric leaf as a scalar cell, matching how the cumulative
/// metrics tsv renders the same value.
fn scalar(value: &Dynamic) -> String {
    cell(&crate::rhai_functions::tracking::dynamic_to_tsv(value))
}

/// Flatten a metric value into `(dotted_suffix, scalar)` leaves.
///
/// `track_freq`/`track_bucket` produce nested maps and `track_unique` produces
/// an array; everything else is already a scalar. The suffix is empty for a
/// scalar, which is what puts an empty key column in the `tsv` form.
fn leaves(value: &Dynamic) -> Vec<(String, String)> {
    if let Some(map) = value.clone().try_cast::<Map>() {
        let mut out = Vec::with_capacity(map.len());
        for (key, inner) in &map {
            let escaped = cell(key.as_str());
            for (suffix, leaf) in leaves(inner) {
                if suffix.is_empty() {
                    out.push((escaped.clone(), leaf));
                } else {
                    out.push((format!("{}.{}", escaped, suffix), leaf));
                }
            }
        }
        return out;
    }

    if let Some(array) = value.clone().try_cast::<rhai::Array>() {
        // track_unique: the window's newly seen values. Rendered as an indexed
        // set rather than a joined string so a value containing the separator
        // cannot be mistaken for two values.
        return array
            .iter()
            .enumerate()
            .map(|(idx, item)| (idx.to_string(), scalar(item)))
            .collect();
    }

    vec![(String::new(), scalar(value))]
}

/// Build one row for a closed span. Never returns an empty string, so callers
/// can push it straight into the output stream.
#[allow(clippy::too_many_arguments)]
pub fn format_row(
    format: &SpanSummaryFormat,
    label: &str,
    span_start: Option<DateTime<Utc>>,
    span_end: Option<DateTime<Utc>>,
    span_id: &str,
    events: i64,
    metrics: &Map,
) -> String {
    match format {
        SpanSummaryFormat::Json => format_json(span_id, span_start, span_end, events, metrics),
        SpanSummaryFormat::Tsv => format_tsv(label, events, metrics),
        // Auto is resolved at config-construction time; treat any leftover as
        // the human shape rather than panicking on a row mid-stream.
        SpanSummaryFormat::Text | SpanSummaryFormat::Auto => format_text(label, events, metrics),
    }
}

fn format_text(label: &str, events: i64, metrics: &Map) -> String {
    let mut out = format!("{}  {}={}", cell(label), EVENTS_KEY, events);
    for (key, value) in metrics {
        let escaped = cell(key.as_str());
        let mut rendered = Vec::new();
        for (suffix, leaf) in leaves(value) {
            if suffix.is_empty() {
                rendered.push(format!("{}={}", escaped, leaf));
            } else {
                rendered.push(format!("{}.{}={}", escaped, suffix, leaf));
            }
        }
        if !rendered.is_empty() {
            out.push_str("  ");
            out.push_str(&rendered.join(" "));
        }
    }
    out
}

fn format_tsv(label: &str, events: i64, metrics: &Map) -> String {
    let label = cell(label);
    let mut lines = vec![format!("{}\t{}\t\t{}", label, EVENTS_KEY, events)];
    for (key, value) in metrics {
        let escaped = cell(key.as_str());
        for (suffix, leaf) in leaves(value) {
            lines.push(format!("{}\t{}\t{}\t{}", label, escaped, suffix, leaf));
        }
    }
    lines.join("\n")
}

fn format_json(
    span_id: &str,
    span_start: Option<DateTime<Utc>>,
    span_end: Option<DateTime<Utc>>,
    events: i64,
    metrics: &Map,
) -> String {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "span".to_string(),
        serde_json::Value::String(span_id.into()),
    );
    if let Some(start) = span_start {
        obj.insert(
            "start".to_string(),
            serde_json::Value::String(start.to_rfc3339_opts(SecondsFormat::Secs, true)),
        );
    }
    if let Some(end) = span_end {
        obj.insert(
            "end".to_string(),
            serde_json::Value::String(end.to_rfc3339_opts(SecondsFormat::Secs, true)),
        );
    }
    obj.insert(EVENTS_KEY.to_string(), serde_json::Value::from(events));

    let mut metric_obj = serde_json::Map::new();
    for (key, value) in metrics {
        metric_obj.insert(
            key.to_string(),
            crate::rhai_functions::tracking::dynamic_to_json(value.clone()),
        );
    }
    obj.insert("metrics".to_string(), serde_json::Value::Object(metric_obj));

    serde_json::Value::Object(obj).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn freq_metrics() -> Map {
        let mut level = Map::new();
        level.insert("DEBUG".into(), Dynamic::from(1_i64));
        level.insert("INFO".into(), Dynamic::from(2_i64));
        let mut metrics = Map::new();
        metrics.insert("level".into(), Dynamic::from(level));
        metrics
    }

    #[test]
    fn label_prefers_start_over_id() {
        assert_eq!(
            span_label("2024-01-15T10:00:00Z/1m", Some(ts(1_705_312_800))),
            "2024-01-15T10:00:00Z"
        );
        assert_eq!(span_label("#0", None), "#0");
        assert_eq!(span_label("api", None), "api");
    }

    #[test]
    fn text_flattens_nested_metrics_with_a_dot() {
        let row = format_text("2024-01-15T10:00:00Z", 3, &freq_metrics());
        assert_eq!(
            row,
            "2024-01-15T10:00:00Z  events=3  level.DEBUG=1 level.INFO=2"
        );
    }

    #[test]
    fn tsv_is_long_form_with_an_empty_key_for_scalars() {
        let row = format_tsv("2024-01-15T10:00:00Z", 3, &freq_metrics());
        assert_eq!(
            row,
            "2024-01-15T10:00:00Z\tevents\t\t3\n\
             2024-01-15T10:00:00Z\tlevel\tDEBUG\t1\n\
             2024-01-15T10:00:00Z\tlevel\tINFO\t2"
        );
    }

    #[test]
    fn json_keeps_metrics_nested() {
        let row = format_json(
            "2024-01-15T10:00:00Z/1m",
            Some(ts(1_705_312_800)),
            Some(ts(1_705_312_860)),
            3,
            &freq_metrics(),
        );
        assert_eq!(
            row,
            r#"{"span":"2024-01-15T10:00:00Z/1m","start":"2024-01-15T10:00:00Z","end":"2024-01-15T10:01:00Z","events":3,"metrics":{"level":{"DEBUG":1,"INFO":2}}}"#
        );
    }

    #[test]
    fn json_omits_start_and_end_when_absent() {
        let row = format_json("#0", None, None, 5, &Map::new());
        assert_eq!(row, r##"{"span":"#0","events":5,"metrics":{}}"##);
    }

    #[test]
    fn tabs_and_newlines_in_labels_cannot_break_a_record() {
        let row = format_tsv("a\tb\nc", 1, &Map::new());
        assert_eq!(row, "a b c\tevents\t\t1");
        assert_eq!(row.lines().count(), 1);
    }

    #[test]
    fn tabs_in_metric_keys_are_sanitized_too() {
        let mut inner = Map::new();
        inner.insert("a\tb".into(), Dynamic::from(2_i64));
        let mut metrics = Map::new();
        metrics.insert("path".into(), Dynamic::from(inner));

        let row = format_tsv("w", 1, &metrics);
        assert!(row.ends_with("w\tpath\ta b\t2"), "{}", row);
        assert_eq!(row.lines().count(), 2);
    }

    #[test]
    fn text_labels_are_sanitized_as_well() {
        let row = format_text("a\tb\nc", 1, &Map::new());
        assert_eq!(row, "a b c  events=1");
        assert_eq!(row.lines().count(), 1);
    }

    #[test]
    fn unique_sets_render_as_indexed_leaves() {
        let mut metrics = Map::new();
        metrics.insert(
            "ips".into(),
            Dynamic::from(vec![Dynamic::from("10.0.0.1".to_string())]),
        );
        assert_eq!(
            format_tsv("w", 1, &metrics),
            "w\tevents\t\t1\nw\tips\t0\t10.0.0.1"
        );
    }

    #[test]
    fn detects_a_user_metric_shadowing_the_events_column() {
        let mut metrics = Map::new();
        metrics.insert("events".into(), Dynamic::from(7_i64));
        assert!(shadows_events_column(&metrics));
        assert!(!shadows_events_column(&freq_metrics()));
    }
}
