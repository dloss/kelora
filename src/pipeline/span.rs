use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use chrono::{DateTime, TimeZone, Utc};
use rhai::Dynamic;

use crate::config::{SpanConfig, SpanMode};
use crate::engine::CompiledExpression;
use crate::event::{Event, SpanInfo, SpanStatus};
use crate::pipeline::PipelineContext;
use crate::platform::{self, SafeStderr};
use crate::rhai_functions::span as span_functions;
use crate::stats;

struct SpanAssignment {
    status: SpanStatus,
    span_id: Option<String>,
    span_start: Option<DateTime<Utc>>,
    span_end: Option<DateTime<Utc>>,
    target_sequence: Option<u64>,
    time_window: Option<TimeWindow>,
}

impl SpanAssignment {
    fn new(status: SpanStatus) -> Self {
        Self {
            status,
            span_id: None,
            span_start: None,
            span_end: None,
            target_sequence: None,
            time_window: None,
        }
    }

    fn with_span(mut self, span: &ActiveSpan) -> Self {
        self.span_id = Some(span.span_id.clone());
        self.span_start = span.span_start;
        self.span_end = span.span_end;
        self.target_sequence = Some(span.sequence);
        self
    }

    fn with_time_window(mut self, window: TimeWindow) -> Self {
        self.span_start = Some(ms_to_datetime(window.start_ms));
        self.span_end = Some(ms_to_datetime(window.end_ms));
        self.span_id = Some(format!(
            "{}/{}",
            self.span_start
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            format_duration(window.duration_ms())
        ));
        self.time_window = Some(window);
        self
    }
}

struct PendingEvent {
    assignment: SpanAssignment,
    applied: usize,
}

impl PendingEvent {
    fn new(assignment: SpanAssignment) -> Self {
        Self {
            assignment,
            applied: 0,
        }
    }

    fn apply_to_event(&mut self, event: &mut Event) {
        event.set_span_info(SpanInfo {
            status: Some(self.assignment.status),
            span_id: self.assignment.span_id.clone(),
            span_start: self.assignment.span_start,
            span_end: self.assignment.span_end,
        });
        self.applied += 1;
    }
}

#[derive(Clone, Copy)]
struct TimeWindow {
    start_ms: i64,
    end_ms: i64,
}

impl TimeWindow {
    fn duration_ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }
}

struct ActiveSpan {
    sequence: u64,
    span_id: String,
    span_start: Option<DateTime<Utc>>,
    span_end: Option<DateTime<Utc>>,
    last_event_timestamp: Option<DateTime<Utc>>,
    events: Vec<Event>,
    events_seen: usize,
    included_count: usize,
    baseline_user: HashMap<String, Dynamic>,
    collect_details: bool,
}

impl ActiveSpan {
    fn new_count(
        sequence: u64,
        index: usize,
        ctx: &PipelineContext,
        collect_details: bool,
    ) -> Self {
        let span_id = format!("#{}", index);
        Self {
            sequence,
            span_id,
            span_start: None,
            span_end: None,
            last_event_timestamp: None,
            events: Vec::new(),
            events_seen: 0,
            included_count: 0,
            baseline_user: if collect_details {
                ctx.tracker.clone()
            } else {
                HashMap::new()
            },
            collect_details,
        }
    }

    fn new_time(
        sequence: u64,
        window: TimeWindow,
        ctx: &PipelineContext,
        collect_details: bool,
    ) -> Self {
        let start = ms_to_datetime(window.start_ms);
        let end = ms_to_datetime(window.end_ms);
        let span_id = format!(
            "{}/{}",
            start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            format_duration(window.duration_ms())
        );

        Self {
            sequence,
            span_id,
            span_start: Some(start),
            span_end: Some(end),
            last_event_timestamp: None,
            events: Vec::new(),
            events_seen: 0,
            included_count: 0,
            baseline_user: if collect_details {
                ctx.tracker.clone()
            } else {
                HashMap::new()
            },
            collect_details,
        }
    }

    fn new_field(
        sequence: u64,
        field_value: String,
        ctx: &PipelineContext,
        collect_details: bool,
    ) -> Self {
        Self {
            sequence,
            span_id: field_value,
            span_start: None,
            span_end: None,
            last_event_timestamp: None,
            events: Vec::new(),
            events_seen: 0,
            included_count: 0,
            baseline_user: if collect_details {
                ctx.tracker.clone()
            } else {
                HashMap::new()
            },
            collect_details,
        }
    }

    fn new_idle(
        sequence: u64,
        start_ts: DateTime<Utc>,
        ctx: &PipelineContext,
        collect_details: bool,
    ) -> Self {
        Self {
            sequence,
            span_id: format!("idle-#{}-{}", sequence, start_ts.to_rfc3339()),
            span_start: Some(start_ts),
            span_end: Some(start_ts),
            last_event_timestamp: Some(start_ts),
            events: Vec::new(),
            events_seen: 0,
            included_count: 0,
            baseline_user: if collect_details {
                ctx.tracker.clone()
            } else {
                HashMap::new()
            },
            collect_details,
        }
    }

    fn note_assignment(&mut self) {
        self.events_seen += 1;
    }

    fn add_event(&mut self, event: &Event) {
        if self.collect_details {
            self.events.push(event.clone());
        }
        self.included_count += 1;
    }
}

pub struct SpanProcessor {
    mode: SpanMode,
    compiled_close: Option<CompiledExpression>,
    collect_details: bool,
    active_span: Option<ActiveSpan>,
    anchor_start_ms: Option<i64>,
    next_count_index: usize,
    next_span_sequence: u64,
    pending: Option<PendingEvent>,
    signal_notice_shown: bool,
    /// Metric keys for which a "no per-window value" warning has already been
    /// emitted, so non-additive aggregators warn once rather than per span.
    warned_non_additive: HashSet<String>,
}

impl SpanProcessor {
    pub fn new(span: SpanConfig, compiled_close: Option<CompiledExpression>) -> Self {
        let SpanConfig { mode, .. } = span;
        let collect_details = compiled_close.is_some();
        Self {
            mode,
            compiled_close,
            collect_details,
            active_span: None,
            anchor_start_ms: None,
            next_count_index: 0,
            next_span_sequence: 0,
            pending: None,
            signal_notice_shown: false,
            warned_non_additive: HashSet::new(),
        }
    }

    pub fn prepare_event(&mut self, event: &mut Event, ctx: &mut PipelineContext) -> Result<()> {
        ctx.meta.span_status = None;
        ctx.meta.span_id = None;
        ctx.meta.span_start = None;
        ctx.meta.span_end = None;
        event.set_span_info(SpanInfo::default());
        self.pending = None;

        match self.mode.clone() {
            SpanMode::Count { events_per_span: _ } => self.prepare_count_event(event, ctx),
            SpanMode::Time { duration_ms } => self.prepare_time_event(event, ctx, duration_ms),
            SpanMode::Field { field_name } => self.prepare_field_event(event, ctx, &field_name),
            SpanMode::Idle { timeout_ms } => self.prepare_idle_event(event, ctx, timeout_ms),
        }
    }

    pub fn prepare_emitted_event(&mut self, event: &mut Event) {
        if let Some(ref mut pending) = self.pending {
            pending.apply_to_event(event);
        }
    }

    pub fn record_emitted_event(&mut self, event: &Event, ctx: &mut PipelineContext) -> Result<()> {
        if let Some(ref mut pending) = self.pending {
            match pending.assignment.status {
                SpanStatus::Included => {
                    if let Some(seq) = pending.assignment.target_sequence {
                        if let Some(ref mut span) = self.active_span {
                            if span.sequence == seq {
                                span.add_event(event);
                                if let SpanMode::Count { events_per_span } = self.mode {
                                    if span.included_count >= events_per_span {
                                        self.close_current_span(ctx)?;
                                    }
                                }
                            }
                        }
                    }
                }
                SpanStatus::Late => {
                    // already counted in prepare_event
                }
                SpanStatus::Unassigned | SpanStatus::Filtered => {}
            }
        }

        Ok(())
    }

    pub fn handle_skip(&mut self, ctx: &mut PipelineContext) {
        if let Some(ref mut pending) = self.pending {
            if pending.assignment.status == SpanStatus::Included {
                ctx.meta.span_status = Some(SpanStatus::Filtered);
            }
        }
    }

    pub fn complete_pending(&mut self) {
        self.pending = None;
    }

    pub fn finish(&mut self, ctx: &mut PipelineContext) -> Result<()> {
        self.pending = None;
        if self.active_span.is_some() {
            self.close_current_span(ctx)?;
        }
        Ok(())
    }

    fn prepare_count_event(&mut self, event: &mut Event, ctx: &mut PipelineContext) -> Result<()> {
        if self.active_span.is_none() {
            self.open_count_span(ctx);
        }

        let span = self
            .active_span
            .as_mut()
            .ok_or_else(|| anyhow!("failed to open count span"))?;
        span.note_assignment();

        let assignment = SpanAssignment::new(SpanStatus::Included).with_span(span);
        self.apply_assignment(event, ctx, &assignment);
        self.pending = Some(PendingEvent::new(assignment));
        Ok(())
    }

    fn prepare_time_event(
        &mut self,
        event: &mut Event,
        ctx: &mut PipelineContext,
        duration_ms: i64,
    ) -> Result<()> {
        if event.parsed_ts.is_none() {
            event.extract_timestamp();
        }

        let timestamp = match event.parsed_ts {
            Some(ts) => ts,
            None => {
                if ctx.config.strict {
                    return Err(anyhow!("event missing required timestamp for --span"));
                }

                let assignment = SpanAssignment::new(SpanStatus::Unassigned);
                self.apply_assignment(event, ctx, &assignment);
                self.pending = Some(PendingEvent::new(assignment));
                return Ok(());
            }
        };

        let event_ms = timestamp.timestamp_millis();
        let window_start = align_to_duration(event_ms, duration_ms);
        let window = TimeWindow {
            start_ms: window_start,
            end_ms: window_start + duration_ms,
        };

        if self.anchor_start_ms.is_none() {
            self.anchor_start_ms = Some(window_start);
        }

        if let Some(ref span) = self.active_span {
            let current_start = span
                .span_start
                .map(|dt| dt.timestamp_millis())
                .expect("time spans must have start timestamp");

            if window_start < current_start {
                let assignment = SpanAssignment::new(SpanStatus::Late).with_time_window(window);
                self.apply_assignment(event, ctx, &assignment);
                self.pending = Some(PendingEvent::new(assignment));
                stats::stats_add_late_event();
                return Ok(());
            }

            if window_start > current_start {
                self.close_current_span(ctx)?;
            }
        }

        if self.active_span.is_none() {
            self.open_time_span(ctx, window);
        }

        let span = self
            .active_span
            .as_mut()
            .ok_or_else(|| anyhow!("failed to open time span"))?;
        span.note_assignment();

        let assignment = SpanAssignment::new(SpanStatus::Included).with_span(span);
        self.apply_assignment(event, ctx, &assignment);
        self.pending = Some(PendingEvent::new(assignment));
        Ok(())
    }

    fn prepare_field_event(
        &mut self,
        event: &mut Event,
        ctx: &mut PipelineContext,
        field_name: &str,
    ) -> Result<()> {
        if let Some(value) = event.fields.get(field_name) {
            let value_str = value.to_string();

            let should_close = match &self.active_span {
                Some(span) => span.span_id != value_str,
                None => false,
            };

            if should_close {
                self.close_current_span(ctx)?;
            }

            if self.active_span.is_none() {
                self.open_field_span(ctx, value_str.clone());
            }

            let span = self
                .active_span
                .as_mut()
                .ok_or_else(|| anyhow!("failed to open field span"))?;
            span.note_assignment();

            let assignment = SpanAssignment::new(SpanStatus::Included).with_span(span);
            self.apply_assignment(event, ctx, &assignment);
            self.pending = Some(PendingEvent::new(assignment));
            return Ok(());
        }

        if ctx.config.strict {
            return Err(anyhow!(
                "event missing required field '{}' for --span",
                field_name
            ));
        }

        if self.active_span.is_none() {
            self.open_field_span(ctx, "(unset)".to_string());
        }

        let span = self
            .active_span
            .as_mut()
            .ok_or_else(|| anyhow!("failed to open field span"))?;
        span.note_assignment();

        let assignment = SpanAssignment::new(SpanStatus::Included).with_span(span);
        self.apply_assignment(event, ctx, &assignment);
        self.pending = Some(PendingEvent::new(assignment));
        Ok(())
    }

    fn prepare_idle_event(
        &mut self,
        event: &mut Event,
        ctx: &mut PipelineContext,
        timeout_ms: i64,
    ) -> Result<()> {
        if event.parsed_ts.is_none() {
            event.extract_timestamp();
        }

        let timestamp = match event.parsed_ts {
            Some(ts) => ts,
            None => {
                if ctx.config.strict {
                    return Err(anyhow!("event missing required timestamp for --span-idle"));
                }

                let assignment = SpanAssignment::new(SpanStatus::Unassigned);
                self.apply_assignment(event, ctx, &assignment);
                self.pending = Some(PendingEvent::new(assignment));
                return Ok(());
            }
        };

        let should_close = match &self.active_span {
            Some(span) => {
                if let Some(last_ts) = span.last_event_timestamp {
                    let gap_ms = timestamp.timestamp_millis() - last_ts.timestamp_millis();
                    gap_ms > timeout_ms
                } else {
                    false
                }
            }
            None => false,
        };

        if should_close {
            self.close_current_span(ctx)?;
        }

        if self.active_span.is_none() {
            self.open_idle_span(ctx, timestamp);
        }

        let span = self
            .active_span
            .as_mut()
            .ok_or_else(|| anyhow!("failed to open idle span"))?;
        span.note_assignment();

        if let Some(last_ts) = span.last_event_timestamp {
            if timestamp > last_ts {
                span.last_event_timestamp = Some(timestamp);
                span.span_end = Some(timestamp);
            }
        } else {
            span.last_event_timestamp = Some(timestamp);
            span.span_end = Some(timestamp);
        }

        let assignment = SpanAssignment::new(SpanStatus::Included).with_span(span);
        self.apply_assignment(event, ctx, &assignment);
        self.pending = Some(PendingEvent::new(assignment));
        Ok(())
    }

    fn apply_assignment(
        &self,
        event: &mut Event,
        ctx: &mut PipelineContext,
        assignment: &SpanAssignment,
    ) {
        ctx.meta.span_status = Some(assignment.status);
        ctx.meta.span_id = assignment.span_id.clone();
        ctx.meta.span_start = assignment.span_start;
        ctx.meta.span_end = assignment.span_end;

        event.set_span_info(SpanInfo {
            status: Some(assignment.status),
            span_id: assignment.span_id.clone(),
            span_start: assignment.span_start,
            span_end: assignment.span_end,
        });
    }

    fn open_count_span(&mut self, ctx: &PipelineContext) {
        let sequence = self.next_span_sequence;
        self.next_span_sequence += 1;
        let index = self.next_count_index;
        self.next_count_index += 1;
        self.active_span = Some(ActiveSpan::new_count(
            sequence,
            index,
            ctx,
            self.collect_details,
        ));
    }

    fn open_field_span(&mut self, ctx: &PipelineContext, field_value: String) {
        let sequence = self.next_span_sequence;
        self.next_span_sequence += 1;
        self.active_span = Some(ActiveSpan::new_field(
            sequence,
            field_value,
            ctx,
            self.collect_details,
        ));
    }

    fn open_time_span(&mut self, ctx: &PipelineContext, window: TimeWindow) {
        let sequence = self.next_span_sequence;
        self.next_span_sequence += 1;
        self.active_span = Some(ActiveSpan::new_time(
            sequence,
            window,
            ctx,
            self.collect_details,
        ));
    }

    fn open_idle_span(&mut self, ctx: &PipelineContext, start_ts: DateTime<Utc>) {
        let sequence = self.next_span_sequence;
        self.next_span_sequence += 1;
        self.active_span = Some(ActiveSpan::new_idle(
            sequence,
            start_ts,
            ctx,
            self.collect_details,
        ));
    }

    fn close_current_span(&mut self, ctx: &mut PipelineContext) -> Result<()> {
        if let Some(mut span) = self.active_span.take() {
            if span.span_end.is_none() {
                span.span_end = span.last_event_timestamp;
            }
            self.run_close_hook(span, ctx)?;
        }
        Ok(())
    }

    fn run_close_hook(&mut self, span: ActiveSpan, ctx: &mut PipelineContext) -> Result<()> {
        if self.compiled_close.is_none() {
            return Ok(());
        }

        let (metrics_delta, non_additive) =
            compute_span_metrics(&span, &ctx.tracker, &ctx.internal_tracker);
        self.warn_non_additive(&non_additive, ctx);

        let compiled = self
            .compiled_close
            .as_ref()
            .expect("compiled_close presence checked above");
        let span_binding = span_functions::SpanBinding::new(
            span.span_id.clone(),
            span.span_start,
            span.span_end,
            &span.events,
            metrics_delta,
        );

        let result = ctx.rhai.execute_compiled_span_close(
            compiled,
            &mut ctx.tracker,
            &mut ctx.internal_tracker,
            span_binding,
        );

        result?;

        if platform::SHOULD_TERMINATE.load(std::sync::atomic::Ordering::Relaxed)
            && !self.signal_notice_shown
        {
            let message = crate::config::format_error_message_auto(
                "Received signal, waiting for span close... (Ctrl+C again to force quit)",
            );
            let _ = SafeStderr::new().writeln(&message);
            self.signal_notice_shown = true;
        }

        Ok(())
    }

    /// Emit a one-time diagnostic for each non-additive metric that cannot be
    /// expressed as a per-window value in `span.metrics`. Without this, such
    /// metrics were silently dropped (avg/percentiles) or reported misleading
    /// global extremes (min/max), giving wrong-or-missing per-window stats with
    /// no indication anything was lost.
    fn warn_non_additive(&mut self, dropped: &[(String, String)], ctx: &PipelineContext) {
        if ctx.config.suppress_warnings || ctx.config.silent {
            return;
        }
        for (key, op) in dropped {
            if !self.warned_non_additive.insert(key.clone()) {
                continue;
            }
            let func = crate::rhai_functions::tracking::op_display_name(op);
            let message = crate::config::format_warning_message_auto(&format!(
                "span.metrics omits '{}' ({}): non-additive aggregators have no per-window \
                 value; iterate span.events to compute per-window min/max/percentiles/etc.",
                key, func
            ));
            let _ = SafeStderr::new().writeln(&message);
        }
    }
}

fn align_to_duration(event_ms: i64, duration_ms: i64) -> i64 {
    (event_ms.div_euclid(duration_ms)) * duration_ms
}

fn ms_to_datetime(ms: i64) -> DateTime<Utc> {
    // Window boundaries are derived from event timestamps plus the span
    // duration (end_ms = window_start + duration_ms). A large --span duration
    // passes the i64-millisecond validation yet pushes a boundary outside
    // chrono's representable range, where timestamp_millis_opt returns None.
    // Clamp to the representable min/max instead of unwrapping (which panicked
    // — aborting the process under the release `panic = "abort"` profile).
    match Utc.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(dt) => dt,
        _ if ms < 0 => DateTime::<Utc>::MIN_UTC,
        _ => DateTime::<Utc>::MAX_UTC,
    }
}

fn format_duration(duration_ms: i64) -> String {
    if duration_ms % 3_600_000 == 0 {
        format!("{}h", duration_ms / 3_600_000)
    } else if duration_ms % 60_000 == 0 {
        format!("{}m", duration_ms / 60_000)
    } else if duration_ms % 1_000 == 0 {
        format!("{}s", duration_ms / 1000)
    } else {
        format!("{}ms", duration_ms)
    }
}

/// Compute `span.metrics` for a closing span.
///
/// Metrics are derived by diffing the global tracker against the baseline
/// captured when the span opened. Only aggregators that compose as a valid
/// per-window value are reported:
///   - `count`/`sum`  -> numeric delta
///   - `avg`          -> (Δsum / Δcount), the true per-window average
///   - `unique`       -> set of values first seen in this window
///   - `bucket`       -> per-bucket count delta
///
/// Non-additive aggregators (`min`, `max`, `percentiles`, `cardinality`,
/// `top`, `bottom`) cannot be recovered from cumulative global state: a global
/// max can't be "un-merged" back to a window max, and a t-digest/HLL has no
/// subtraction. These are collected into the returned list so the caller can
/// emit a diagnostic instead of silently dropping them (or, for min/max,
/// reporting a misleading global extreme).
fn compute_span_metrics(
    span: &ActiveSpan,
    current_user: &HashMap<String, Dynamic>,
    current_internal: &HashMap<String, Dynamic>,
) -> (rhai::Map, Vec<(String, String)>) {
    let mut result = rhai::Map::new();
    let mut non_additive: Vec<(String, String)> = Vec::new();

    for (key, value) in current_user {
        let op_key = format!("__op_{}", key);
        let operation = current_internal
            .get(&op_key)
            .and_then(|v| v.clone().into_string().ok());

        if let Some(op) = operation.as_deref() {
            match op {
                "count" | "sum" => {
                    if let Some(delta) = numeric_delta(value, span.baseline_user.get(key)) {
                        if !is_zero_dynamic(&delta) {
                            result.insert(key.clone().into(), delta);
                        }
                    }
                }
                "avg" => {
                    if let Some(avg) = avg_delta(value, span.baseline_user.get(key)) {
                        result.insert(key.clone().into(), avg);
                    }
                }
                "unique" => {
                    if let Some(delta) = unique_delta(value, span.baseline_user.get(key)) {
                        if !delta.is_empty() {
                            result.insert(key.clone().into(), Dynamic::from(delta));
                        }
                    }
                }
                "bucket" => {
                    if let Some(delta) = bucket_delta(value, span.baseline_user.get(key)) {
                        if !delta.is_empty() {
                            result.insert(key.clone().into(), Dynamic::from(delta));
                        }
                    }
                }
                "min" | "max" | "percentiles" | "cardinality" | "top" | "bottom" | "top_by"
                | "bottom_by" => {
                    non_additive.push((key.clone(), op.to_string()));
                }
                _ => {}
            }
        }
    }

    (result, non_additive)
}

/// Extract the cumulative `(sum, count)` stored by `track_avg`.
fn extract_avg_parts(value: &Dynamic) -> Option<(f64, i64)> {
    let map = value.clone().try_cast::<rhai::Map>()?;
    let sum = map.get("sum").and_then(|v| {
        if v.is_float() {
            v.as_float().ok()
        } else if v.is_int() {
            v.as_int().ok().map(|i| i as f64)
        } else {
            None
        }
    })?;
    let count = map.get("count").and_then(|v| v.as_int().ok())?;
    Some((sum, count))
}

/// Compute the per-window average from cumulative `{sum, count}` snapshots.
/// Because `track_avg` stores running totals, the windowed average is exactly
/// `(sum_now - sum_base) / (count_now - count_base)`. Returns `None` when no
/// values were tracked in this window.
fn avg_delta(current: &Dynamic, base: Option<&Dynamic>) -> Option<Dynamic> {
    let (cur_sum, cur_count) = extract_avg_parts(current)?;
    let (base_sum, base_count) = base.and_then(extract_avg_parts).unwrap_or((0.0, 0));
    let delta_count = cur_count - base_count;
    if delta_count <= 0 {
        return None;
    }
    let delta_sum = cur_sum - base_sum;
    Some(Dynamic::from(delta_sum / delta_count as f64))
}

fn numeric_delta(current: &Dynamic, base: Option<&Dynamic>) -> Option<Dynamic> {
    let current_val = if current.is_float() {
        current.as_float().ok()?
    } else {
        current.as_int().ok()? as f64
    };

    let base_val = base
        .and_then(|b| {
            if b.is_float() {
                b.as_float().ok()
            } else {
                b.as_int().ok().map(|v| v as f64)
            }
        })
        .unwrap_or(0.0);

    let delta = current_val - base_val;
    Some(if current.is_float() {
        Dynamic::from(delta)
    } else {
        Dynamic::from(delta.round() as i64)
    })
}

fn unique_delta(current: &Dynamic, base: Option<&Dynamic>) -> Option<rhai::Array> {
    let current_arr = current.clone().into_array().ok()?;
    let base_arr = base
        .and_then(|b| b.clone().into_array().ok())
        .unwrap_or_default();

    let base_set: std::collections::HashSet<String> = base_arr
        .iter()
        .filter_map(|v| v.clone().into_string().ok())
        .collect();

    let mut diff = rhai::Array::new();
    for item in current_arr {
        let key = item
            .clone()
            .into_string()
            .unwrap_or_else(|_| item.to_string());
        if !base_set.contains(&key) {
            diff.push(item);
        }
    }

    Some(diff)
}

fn bucket_delta(current: &Dynamic, base: Option<&Dynamic>) -> Option<rhai::Map> {
    let current_map = current.clone().try_cast::<rhai::Map>()?;
    let base_map = base
        .and_then(|b| b.clone().try_cast::<rhai::Map>())
        .unwrap_or_default();

    let mut diff = rhai::Map::new();

    for (bucket, value) in current_map {
        let current_count = value.as_int().unwrap_or(0);
        let base_count = base_map
            .get(&bucket)
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0);
        let delta = current_count - base_count;
        if delta > 0 {
            diff.insert(bucket, Dynamic::from(delta));
        }
    }

    Some(diff)
}

fn is_zero_dynamic(value: &Dynamic) -> bool {
    if value.is_float() {
        value.as_float().unwrap_or(0.0).abs() < f64::EPSILON
    } else if value.is_int() {
        value.as_int().unwrap_or(0) == 0
    } else {
        false
    }
}
