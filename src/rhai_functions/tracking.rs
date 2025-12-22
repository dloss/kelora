use crate::stats::ProcessingStats;
use rhai::{Dynamic, Engine};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tdigests::TDigest;

/// Snapshot of tracking state separated into user-visible metrics and internal-only data.
#[derive(Debug, Clone, Default)]
pub struct TrackingSnapshot {
    pub user: HashMap<String, Dynamic>,
    pub internal: HashMap<String, Dynamic>,
}

impl TrackingSnapshot {
    pub fn from_parts(user: HashMap<String, Dynamic>, internal: HashMap<String, Dynamic>) -> Self {
        Self { user, internal }
    }
}

// Thread-local storage for tracking state
thread_local! {
    pub static THREAD_TRACKING_STATE: RefCell<TrackingSnapshot> = RefCell::new(TrackingSnapshot::default());
}

pub fn get_thread_snapshot() -> TrackingSnapshot {
    THREAD_TRACKING_STATE.with(|state| state.borrow().clone())
}

pub fn with_user_tracking<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<String, Dynamic>) -> R,
{
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        f(&mut snapshot.user)
    })
}

pub fn with_internal_tracking<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<String, Dynamic>) -> R,
{
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        f(&mut snapshot.internal)
    })
}

fn record_operation_metadata(key: &str, operation: &str) {
    with_internal_tracking(|internal| {
        internal.insert(
            format!("__op_{}", key),
            Dynamic::from(operation.to_string()),
        );
    });
}

pub fn set_thread_tracking_state(metrics: &HashMap<String, Dynamic>) {
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        snapshot.user = metrics.clone();
    });
}

pub fn get_thread_tracking_state() -> HashMap<String, Dynamic> {
    THREAD_TRACKING_STATE.with(|state| state.borrow().user.clone())
}

pub fn set_thread_internal_state(metrics: &HashMap<String, Dynamic>) {
    THREAD_TRACKING_STATE.with(|state| {
        let mut snapshot = state.borrow_mut();
        snapshot.internal = metrics.clone();
    });
}

pub fn get_thread_internal_state() -> HashMap<String, Dynamic> {
    THREAD_TRACKING_STATE.with(|state| state.borrow().internal.clone())
}

fn merge_numeric(existing: Option<Dynamic>, new_value: Dynamic) -> Dynamic {
    let new_is_float = new_value.is_float();

    if let Some(current) = existing {
        let current_is_float = current.is_float();

        if current_is_float || new_is_float {
            let current_total = if current_is_float {
                current.as_float().unwrap_or(0.0)
            } else {
                current.as_int().unwrap_or(0) as f64
            };

            let incoming = if new_is_float {
                new_value.as_float().unwrap_or(0.0)
            } else {
                new_value.as_int().unwrap_or(0) as f64
            };

            Dynamic::from(current_total + incoming)
        } else {
            let current_total = current.as_int().unwrap_or(0);
            let incoming = new_value.as_int().unwrap_or(0);
            Dynamic::from(current_total + incoming)
        }
    } else {
        new_value
    }
}

/// Format filename for error display based on input context
/// Returns appropriate display format: line number only for single file/stdin,
/// basename for multiple files without conflicts, full path for conflicts
fn format_error_location(
    line_num: Option<usize>,
    filename: Option<&str>,
    input_files: &[String],
) -> String {
    match (line_num, filename) {
        (Some(line), Some(fname)) => {
            // Check if we have single file or stdin
            if input_files.is_empty() || input_files.len() == 1 {
                format!("line {}", line)
            } else {
                // Multiple files - check for basename conflicts
                let basenames: HashSet<_> = input_files
                    .iter()
                    .map(|f| {
                        Path::new(f)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                    })
                    .collect();

                if basenames.len() == input_files.len() {
                    // No conflicts - use basename
                    let basename = Path::new(fname)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");
                    format!("{}:{}", basename, line)
                } else {
                    // Conflicts exist - use full path
                    format!("{}:{}", fname, line)
                }
            }
        }
        (Some(line), None) => format!("line {}", line),
        _ => "unknown".to_string(),
    }
}

/// Unified error tracking function that handles counts, samples, and verbose output
/// This replaces both stats-based and tracking-based error mechanisms
/// Note: This function has 8 parameters because it needs to handle diverse error contexts:
/// location info (line_num, filename), content (message, original_line), error classification (error_type),
/// and output control (verbose, quiet, config). Grouping these would add complexity without benefit.
#[allow(clippy::too_many_arguments)]
pub fn track_error(
    error_type: &str,
    line_num: Option<usize>,
    message: &str,
    original_line: Option<&str>,
    filename: Option<&str>,
    verbose: u8,
    quiet_level: u8,
    config: Option<&crate::pipeline::PipelineConfig>,
    format_name: Option<&str>,
) {
    // Use tracking infrastructure for error counting in all modes
    // This ensures consistent mapreduce behavior and avoids double counting

    with_internal_tracking(|state| {
        // Track error count by type - uses "count" operation for summing across workers
        let count_key = format!("__kelora_error_count_{}", error_type);
        let current_count = state
            .get(&count_key)
            .cloned()
            .unwrap_or(Dynamic::from(0i64));
        let new_count = current_count.as_int().unwrap_or(0) + 1;
        state.insert(count_key.clone(), Dynamic::from(new_count));
        state.insert(format!("__op_{}", count_key), Dynamic::from("count"));

        // Output verbose errors - use ordered capture system for proper interleaving
        if verbose > 0 && quiet_level == 0 {
            // Enhanced format with filename for immediate verbose output
            // Determine emoji usage
            let use_emoji = if let Some(cfg) = config {
                crate::tty::should_use_emoji_with_mode(&cfg.emoji_mode, &cfg.color_mode)
            } else {
                crate::tty::should_use_emoji_for_stderr()
            };
            let prefix = if use_emoji { "⚠️ " } else { "kelora: " };

            let input_files = config.map(|c| c.input_files.as_slice()).unwrap_or(&[]);

            let location = format_error_location(line_num, filename, input_files);
            let mut formatted_error = if error_type == "parse" {
                // For parse errors, include format name if available
                let format_info = if let Some(fmt) = format_name {
                    format!(" (format: {})", fmt)
                } else {
                    String::new()
                };

                if !location.is_empty() && location != "unknown" {
                    format!("{}{}{}: {}", prefix, location, format_info, message)
                } else {
                    format!("{}{}{}", prefix, format_info.trim_start(), message)
                }
            } else {
                // For other error types, keep the existing format
                if !location.is_empty() && location != "unknown" {
                    format!("{}{}: {} - {}", prefix, location, error_type, message)
                } else {
                    format!("{}{} - {}", prefix, error_type, message)
                }
            };

            // Add preprocessing hint for parse errors when format is known
            if error_type == "parse" && format_name.is_some() && verbose > 0 {
                let hint = "\n  Hint: Input may contain mixed formats. Consider preprocessing:\n    - Split by format: grep '^{' input.log | kelora -f json\n    - Use multiline detection: kelora -M 'regex:match=^{' -f json";
                formatted_error.push_str(hint);
            }

            if crate::rhai_functions::strings::is_parallel_mode() {
                // In parallel mode, capture stderr message for ordered output later
                crate::rhai_functions::strings::capture_stderr(formatted_error.clone());
                // Show original line content for verbose >= 2 (-vv) - only for parse errors
                if verbose >= 2 && error_type == "parse" {
                    if let Some(line) = original_line {
                        crate::rhai_functions::strings::capture_stderr(format!("    {}", line));
                        // Show additional line details for verbose >= 3 (-vvv)
                        if verbose >= 3 {
                            let non_ascii_count = line.chars().filter(|c| !c.is_ascii()).count();
                            let control_char_count = line
                                .chars()
                                .filter(|c| {
                                    c.is_control() && *c != '\t' && *c != '\n' && *c != '\r'
                                })
                                .count();
                            let line_info = format!("    (length: {} chars, non_ascii: {}, control_chars: {}, starts: {:?}, ends: {:?})",
                                line.len(),
                                non_ascii_count,
                                control_char_count,
                                line.chars().next().unwrap_or('\0'),
                                line.chars().last().unwrap_or('\0')
                            );
                            crate::rhai_functions::strings::capture_stderr(line_info);
                        }
                    }
                }
            } else {
                // In sequential mode, output immediately but also capture for consistency
                crate::rhai_functions::strings::capture_stderr(formatted_error.clone());
                eprintln!("{}", formatted_error);
                // Show original line content for verbose >= 2 (-vv) - only for parse errors
                if verbose >= 2 && error_type == "parse" {
                    if let Some(line) = original_line {
                        let indented_line = format!("    {}", line);
                        crate::rhai_functions::strings::capture_stderr(indented_line.clone());
                        eprintln!("{}", indented_line);
                        // Show additional line details for verbose >= 3 (-vvv)
                        if verbose >= 3 {
                            let non_ascii_count = line.chars().filter(|c| !c.is_ascii()).count();
                            let control_char_count = line
                                .chars()
                                .filter(|c| {
                                    c.is_control() && *c != '\t' && *c != '\n' && *c != '\r'
                                })
                                .count();
                            let line_info = format!("    (length: {} chars, non_ascii: {}, control_chars: {}, starts: {:?}, ends: {:?})",
                                line.len(),
                                non_ascii_count,
                                control_char_count,
                                line.chars().next().unwrap_or('\0'),
                                line.chars().last().unwrap_or('\0')
                            );
                            crate::rhai_functions::strings::capture_stderr(line_info.clone());
                            eprintln!("{}", line_info);
                        }
                    }
                }
            }
        }

        // Track error samples (max 3 per type) - uses "unique" operation for deduplication
        // This stores examples for display but doesn't affect the total count
        let samples_key = format!("__kelora_error_samples_{}", error_type);
        let current_samples = state
            .get(&samples_key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current_samples.into_array() {
            // Only store up to 3 samples per error type
            if arr.len() < 3 {
                // Create a sample object containing error details and original line
                let mut sample_obj = rhai::Map::new();
                sample_obj.insert("error_type".into(), Dynamic::from(error_type.to_string()));
                sample_obj.insert(
                    "line_num".into(),
                    Dynamic::from(line_num.unwrap_or(0) as i64),
                );
                sample_obj.insert("message".into(), Dynamic::from(message.to_string()));
                if let Some(line) = original_line {
                    sample_obj.insert("original_line".into(), Dynamic::from(line.to_string()));
                }
                // Store filename if available
                if let Some(filename) = filename {
                    sample_obj.insert("filename".into(), Dynamic::from(filename.to_string()));
                }

                arr.push(Dynamic::from(sample_obj));
            }

            state.insert(samples_key.clone(), Dynamic::from(arr));
            state.insert(format!("__op_{}", samples_key), Dynamic::from("unique"));
        }
    });
}

/// Check if any errors occurred based on tracking data
pub fn has_errors_in_tracking(snapshot: &TrackingSnapshot) -> bool {
    for (key, value) in &snapshot.internal {
        if let Some(_error_type) = key.strip_prefix("__kelora_error_count_") {
            if let Ok(count) = value.as_int() {
                if count > 0 {
                    return true;
                }
            }
        }
    }
    false
}

/// Format a concise single-line fatal error summary for --silent mode
/// Uses smart hybrid approach: adapts based on error count and type
pub fn format_fatal_error_line(snapshot: &TrackingSnapshot) -> String {
    let mut total_errors = 0;
    let mut error_types = Vec::new();
    let mut all_samples: Vec<rhai::Map> = Vec::new();

    // Collect error counts by type
    for (key, value) in &snapshot.internal {
        if let Some(error_type) = key.strip_prefix("__kelora_error_count_") {
            if let Ok(count) = value.as_int() {
                if count > 0 {
                    total_errors += count;
                    error_types.push((error_type.to_string(), count));
                }
            }
        }
    }

    if total_errors == 0 {
        return "fatal error encountered".to_string();
    }

    // Collect sample objects for context
    for (key, value) in &snapshot.internal {
        if let Some(_error_type) = key.strip_prefix("__kelora_error_samples_") {
            if let Ok(sample_array) = value.clone().into_array() {
                for sample in sample_array {
                    if let Some(sample_map) = sample.try_cast::<rhai::Map>() {
                        all_samples.push(sample_map);
                    }
                }
            }
        }
    }

    // Single error type
    if error_types.len() == 1 {
        let (error_type, count) = &error_types[0];

        if *count == 1 && !all_samples.is_empty() {
            // Single error: show message
            let sample = &all_samples[0];
            let message = sample
                .get("message")
                .and_then(|v| v.clone().into_string().ok())
                .unwrap_or_else(|| "unknown error".to_string());

            // Truncate very long messages for single line
            let message = if message.len() > 80 {
                format!("{}...", &message[..77])
            } else {
                message
            };

            format!("1 {} error: {}", error_type, message)
        } else if *count <= 3 && all_samples.len() as i64 == *count {
            // Few errors: show line numbers for all
            let lines: Vec<String> = all_samples
                .iter()
                .map(|s| {
                    s.get("line_num")
                        .and_then(|v| v.as_int().ok())
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "?".to_string())
                })
                .collect();

            format!(
                "{} {} errors at lines {}",
                count,
                error_type,
                lines.join(", ")
            )
        } else if !all_samples.is_empty() {
            // More errors: show count + first error location
            let first_line = all_samples[0]
                .get("line_num")
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0);

            format!(
                "{} {} errors (first at line {})",
                count, error_type, first_line
            )
        } else {
            // No samples available: just show count
            format!("{} {} errors", count, error_type)
        }
    } else {
        // Mixed error types
        if total_errors <= 10 {
            // Show breakdown for small counts
            let breakdown: Vec<String> = error_types
                .iter()
                .map(|(t, c)| format!("{} {}", c, t))
                .collect();

            format!("{} errors: {}", total_errors, breakdown.join(", "))
        } else {
            // Large mixed counts
            format!("{} errors (mixed types)", total_errors)
        }
    }
}

/// Extract error summary from tracking state with different verbosity levels
pub fn extract_error_summary_from_tracking(
    snapshot: &TrackingSnapshot,
    verbose: u8,
    stats: Option<&ProcessingStats>,
    _config: Option<&crate::config::KeloraConfig>,
) -> Option<String> {
    let mut total_errors = 0;
    let mut error_types = Vec::new();
    let mut sample_objects: Vec<rhai::Map> = Vec::new();

    // Collect error counts by type
    for (key, value) in &snapshot.internal {
        if let Some(error_type) = key.strip_prefix("__kelora_error_count_") {
            if let Ok(count) = value.as_int() {
                if count > 0 {
                    total_errors += count;
                    error_types.push((error_type.to_string(), count));
                }
            }
        }
    }

    if total_errors == 0 {
        return None;
    }

    // Collect sample objects with structured data
    for (key, value) in &snapshot.internal {
        if let Some(_error_type) = key.strip_prefix("__kelora_error_samples_") {
            if let Ok(sample_array) = value.clone().into_array() {
                for sample in sample_array {
                    if let Some(sample_map) = sample.try_cast::<rhai::Map>() {
                        sample_objects.push(sample_map);
                    }
                }
            }
        }
    }

    // Use the new concise format
    let mut summary = String::new();

    // Determine primary error type for header
    let primary_error_type = if error_types.len() == 1 {
        &error_types[0].0
    } else {
        // For mixed types, use generic "errors"
        "mixed"
    };

    // Format header: "Parse errors: N total" or "Mixed errors: N total"
    if primary_error_type == "mixed" {
        summary.push_str(&format!("Mixed errors: {} total", total_errors));
    } else {
        summary.push_str(&format!(
            "{}{} errors: {} total",
            primary_error_type.chars().next().unwrap().to_uppercase(),
            &primary_error_type[1..],
            total_errors
        ));
    }

    // Show up to 3 examples with filename:line format
    let mut shown_samples = 0;
    for sample_obj in &sample_objects {
        if shown_samples >= 3 {
            break;
        }

        // Extract data from sample object
        let line_num = sample_obj
            .get("line_num")
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0);
        let message = sample_obj
            .get("message")
            .and_then(|v| v.clone().into_string().ok())
            .unwrap_or_else(|| "unknown error".to_string());
        let filename = sample_obj
            .get("filename")
            .and_then(|v| v.clone().into_string().ok())
            .unwrap_or_else(|| "stdin".to_string());
        let original_line = sample_obj
            .get("original_line")
            .and_then(|v| v.clone().into_string().ok());

        // Format location using same smart logic as immediate errors
        let input_files = &[];

        let location = format_error_location(Some(line_num as usize), Some(&filename), input_files);

        summary.push_str(&format!("\n  {}: {}", location, message));

        // In verbose mode, add indented original line
        if verbose > 0 {
            if let Some(orig_line) = original_line {
                // Truncate very long lines for readability
                let display_line = if orig_line.len() > 100 {
                    format!("{}...", &orig_line[..97])
                } else {
                    orig_line
                };
                summary.push_str(&format!("\n    {}", display_line));
            }
        }

        shown_samples += 1;
    }

    // Add "more errors not shown" if total errors exceed samples shown
    if total_errors as usize > shown_samples {
        let remaining = total_errors as usize - shown_samples;
        let message = if verbose > 0 {
            "All errors shown during processing. Use --no-diagnostics to suppress this summary."
        } else {
            "Use -v to see each error or --no-diagnostics to suppress this summary."
        };

        summary.push_str(&format!("\n  [+{} more. {}]", remaining, message));
    }

    if let Some(stats) = stats {
        if stats.yearless_timestamps > 0 {
            let warning_msg = format!(
                "Year-less timestamp format detected ({} parse{})\n\
                   Format lacks year (e.g., \"Dec 31 23:59:59\")\n\
                   Year inferred using heuristic (+/- 1 year from current date)\n\
                   Timestamps >18 months old may be incorrect",
                stats.yearless_timestamps,
                if stats.yearless_timestamps == 1 {
                    ""
                } else {
                    "s"
                }
            );
            summary.push_str("\n  ");
            summary.push_str(
                &crate::config::format_warning_message_auto(&warning_msg).replace('\n', "\n  "),
            );
        }
    }

    Some(summary)
}

/// Helper function to serialize a TDigest to bytes for storage in Dynamic
/// We store centroids as the serialization format
fn serialize_tdigest(digest: &TDigest) -> Vec<u8> {
    let centroids = digest.centroids();
    let mut bytes = Vec::new();

    // Store number of centroids (8 bytes)
    let count = centroids.len();
    bytes.extend_from_slice(&count.to_le_bytes());

    // Store each centroid (mean: f64, weight: f64 = 16 bytes each)
    for centroid in centroids {
        bytes.extend_from_slice(&centroid.mean.to_le_bytes());
        bytes.extend_from_slice(&centroid.weight.to_le_bytes());
    }

    bytes
}

/// Helper function to deserialize a TDigest from bytes stored in Dynamic
fn deserialize_tdigest(bytes: &[u8]) -> Option<TDigest> {
    if bytes.len() < 8 {
        return None;
    }

    // Read number of centroids
    let count = usize::from_le_bytes(bytes[0..8].try_into().ok()?);

    if bytes.len() < 8 + count * 16 {
        return None;
    }

    // Reconstruct centroids
    let mut centroids = Vec::with_capacity(count);
    for i in 0..count {
        let offset = 8 + i * 16;
        let mean = f64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
        let weight = f64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().ok()?);
        centroids.push(tdigests::Centroid::new(mean, weight));
    }

    // Reconstruct t-digest from centroids
    Some(TDigest::from_centroids(centroids))
}

/// Implementation of track_percentiles for a given numeric type
fn track_percentiles_impl(
    key: &str,
    value: f64,
    percentiles: rhai::Array,
) -> Result<(), Box<rhai::EvalAltResult>> {
    // Validate percentiles array is not empty
    if percentiles.is_empty() {
        return Err("track_percentiles requires a non-empty array of percentiles".into());
    }

    // Parse and validate percentiles
    let mut valid_percentiles = Vec::new();
    let mut seen = HashSet::new();

    for p in percentiles {
        let percentile = if p.is_int() {
            p.as_int().map_err(|_| -> Box<rhai::EvalAltResult> {
                "track_percentiles percentile must be a number".into()
            })? as f64
        } else if p.is_float() {
            p.as_float().map_err(|_| -> Box<rhai::EvalAltResult> {
                "track_percentiles percentile must be a number".into()
            })?
        } else {
            return Err("track_percentiles percentile must be a number".into());
        };

        // Validate range [0, 100]
        if !(0.0..=100.0).contains(&percentile) {
            return Err(format!(
                "track_percentiles percentile must be in range [0, 100], got {}",
                percentile
            )
            .into());
        }

        // Deduplicate
        if !seen.contains(&percentile.to_bits()) {
            seen.insert(percentile.to_bits());
            valid_percentiles.push(percentile);
        }
    }

    // Filter out NaN and Infinity
    if !value.is_finite() {
        // Silently skip invalid values (like track_min does with Unit)
        return Ok(());
    }

    // Track each percentile independently (auto-suffixing behavior)
    for percentile in valid_percentiles {
        // Format percentile as integer if whole number, otherwise as decimal
        let percentile_str = if percentile.fract() == 0.0 {
            format!("p{}", percentile as i64)
        } else {
            format!("p{}", percentile)
        };

        let metric_key = format!("{}_{}", key, percentile_str);

        with_user_tracking(|state| {
            // Create a new digest with just this value
            let new_digest = TDigest::from_values(vec![value]);

            // Get existing digest or use the new one
            let digest = if let Some(existing) = state.get(&metric_key) {
                // Try to deserialize existing digest and merge
                if let Ok(bytes) = existing.clone().into_blob() {
                    if let Some(existing_digest) = deserialize_tdigest(&bytes) {
                        existing_digest.merge(&new_digest)
                    } else {
                        new_digest
                    }
                } else {
                    new_digest
                }
            } else {
                new_digest
            };

            // Serialize and store
            let bytes = serialize_tdigest(&digest);
            state.insert(metric_key.clone(), Dynamic::from_blob(bytes));
        });

        // Record operation metadata for parallel merging
        record_operation_metadata(&metric_key, "percentiles");
    }

    Ok(())
}

pub fn register_functions(engine: &mut Engine) {
    // Track functions using thread-local storage - clean user API
    // Store operation metadata for proper parallel merging
    engine.register_fn(
        "track_count",
        |key: Dynamic| -> Result<(), Box<rhai::EvalAltResult>> {
            let type_name = key.type_name().to_string();
            let key = key.into_string().map_err(|_| -> Box<rhai::EvalAltResult> {
                format!(
                    "track_count requires a string key; got {}. Hint: use to_string() for numbers (e.g. track_count(e.status.to_string()))",
                    type_name
                )
                .into()
            })?;

            with_user_tracking(|state| {
                let updated =
                    merge_numeric(state.get(key.as_str()).cloned(), Dynamic::from(1_i64));
                state.insert(key.to_string(), updated);
            });
            record_operation_metadata(&key, "count");
            Ok(())
        },
    );

    engine.register_fn("track_sum", |key: &str, value: i64| {
        with_user_tracking(|state| {
            let updated = merge_numeric(state.get(key).cloned(), Dynamic::from(value));
            state.insert(key.to_string(), updated);
        });
        record_operation_metadata(key, "sum");
    });

    engine.register_fn("track_sum", |key: &str, value: i32| {
        with_user_tracking(|state| {
            let updated = merge_numeric(state.get(key).cloned(), Dynamic::from(value));
            state.insert(key.to_string(), updated);
        });
        record_operation_metadata(key, "sum");
    });

    engine.register_fn("track_sum", |key: &str, value: f64| {
        with_user_tracking(|state| {
            let updated = merge_numeric(state.get(key).cloned(), Dynamic::from(value));
            state.insert(key.to_string(), updated);
        });
        record_operation_metadata(key, "sum");
    });

    engine.register_fn("track_sum", |key: &str, value: f32| {
        with_user_tracking(|state| {
            let updated = merge_numeric(state.get(key).cloned(), Dynamic::from(value));
            state.insert(key.to_string(), updated);
        });
        record_operation_metadata(key, "sum");
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_sum", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_avg overloads for different number types
    // Stores both sum and count as a map for proper averaging in parallel mode
    engine.register_fn("track_avg", |key: &str, value: i64| {
        with_user_tracking(|state| {
            let current = state.get(key).cloned();
            let (new_sum, new_count) = if let Some(existing) = current {
                // Try to extract existing map with sum and count
                if let Some(map) = existing.try_cast::<rhai::Map>() {
                    let existing_sum = map
                        .get("sum")
                        .and_then(|v| {
                            if v.is_float() {
                                v.as_float().ok()
                            } else if v.is_int() {
                                v.as_int().ok().map(|i| i as f64)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0.0);
                    let existing_count =
                        map.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                    (existing_sum + value as f64, existing_count + 1)
                } else {
                    // Legacy case: if existing is just a number, treat it as sum with count 1
                    (value as f64, 1)
                }
            } else {
                (value as f64, 1)
            };

            let mut map = rhai::Map::new();
            map.insert("sum".into(), Dynamic::from(new_sum));
            map.insert("count".into(), Dynamic::from(new_count));
            state.insert(key.to_string(), Dynamic::from(map));
        });
        record_operation_metadata(key, "avg");
    });

    engine.register_fn("track_avg", |key: &str, value: i32| {
        with_user_tracking(|state| {
            let current = state.get(key).cloned();
            let (new_sum, new_count) = if let Some(existing) = current {
                if let Some(map) = existing.try_cast::<rhai::Map>() {
                    let existing_sum = map
                        .get("sum")
                        .and_then(|v| {
                            if v.is_float() {
                                v.as_float().ok()
                            } else if v.is_int() {
                                v.as_int().ok().map(|i| i as f64)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0.0);
                    let existing_count =
                        map.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                    (existing_sum + value as f64, existing_count + 1)
                } else {
                    (value as f64, 1)
                }
            } else {
                (value as f64, 1)
            };

            let mut map = rhai::Map::new();
            map.insert("sum".into(), Dynamic::from(new_sum));
            map.insert("count".into(), Dynamic::from(new_count));
            state.insert(key.to_string(), Dynamic::from(map));
        });
        record_operation_metadata(key, "avg");
    });

    engine.register_fn("track_avg", |key: &str, value: f64| {
        with_user_tracking(|state| {
            let current = state.get(key).cloned();
            let (new_sum, new_count) = if let Some(existing) = current {
                if let Some(map) = existing.try_cast::<rhai::Map>() {
                    let existing_sum = map
                        .get("sum")
                        .and_then(|v| {
                            if v.is_float() {
                                v.as_float().ok()
                            } else if v.is_int() {
                                v.as_int().ok().map(|i| i as f64)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0.0);
                    let existing_count =
                        map.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                    (existing_sum + value, existing_count + 1)
                } else {
                    (value, 1)
                }
            } else {
                (value, 1)
            };

            let mut map = rhai::Map::new();
            map.insert("sum".into(), Dynamic::from(new_sum));
            map.insert("count".into(), Dynamic::from(new_count));
            state.insert(key.to_string(), Dynamic::from(map));
        });
        record_operation_metadata(key, "avg");
    });

    engine.register_fn("track_avg", |key: &str, value: f32| {
        with_user_tracking(|state| {
            let current = state.get(key).cloned();
            let (new_sum, new_count) = if let Some(existing) = current {
                if let Some(map) = existing.try_cast::<rhai::Map>() {
                    let existing_sum = map
                        .get("sum")
                        .and_then(|v| {
                            if v.is_float() {
                                v.as_float().ok()
                            } else if v.is_int() {
                                v.as_int().ok().map(|i| i as f64)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0.0);
                    let existing_count =
                        map.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                    (existing_sum + value as f64, existing_count + 1)
                } else {
                    (value as f64, 1)
                }
            } else {
                (value as f64, 1)
            };

            let mut map = rhai::Map::new();
            map.insert("sum".into(), Dynamic::from(new_sum));
            map.insert("count".into(), Dynamic::from(new_count));
            state.insert(key.to_string(), Dynamic::from(map));
        });
        record_operation_metadata(key, "avg");
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_avg", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_min overloads for different number types
    engine.register_fn("track_min", |key: &str, value: i64| {
        let updated = with_user_tracking(|state| {
            let current = state
                .get(key)
                .cloned()
                .unwrap_or(Dynamic::from(f64::INFINITY));
            let current_val = if current.is_int() {
                current.as_int().unwrap_or(i64::MAX) as f64
            } else {
                current.as_float().unwrap_or(f64::INFINITY)
            };
            let value_f64 = value as f64;
            if value_f64 < current_val {
                state.insert(key.to_string(), Dynamic::from(value));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "min");
        }
    });

    engine.register_fn("track_min", |key: &str, value: i32| {
        let updated = with_user_tracking(|state| {
            let current = state
                .get(key)
                .cloned()
                .unwrap_or(Dynamic::from(f64::INFINITY));
            let current_val = if current.is_int() {
                current.as_int().unwrap_or(i64::MAX) as f64
            } else {
                current.as_float().unwrap_or(f64::INFINITY)
            };
            let value_f64 = value as f64;
            if value_f64 < current_val {
                state.insert(key.to_string(), Dynamic::from(value));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "min");
        }
    });

    engine.register_fn("track_min", |key: &str, value: f64| {
        let updated = with_user_tracking(|state| {
            let current = state
                .get(key)
                .cloned()
                .unwrap_or(Dynamic::from(f64::INFINITY));
            let current_val = if current.is_int() {
                current.as_int().unwrap_or(i64::MAX) as f64
            } else {
                current.as_float().unwrap_or(f64::INFINITY)
            };
            if value < current_val {
                state.insert(key.to_string(), Dynamic::from(value));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "min");
        }
    });

    engine.register_fn("track_min", |key: &str, value: f32| {
        let updated = with_user_tracking(|state| {
            let current = state
                .get(key)
                .cloned()
                .unwrap_or(Dynamic::from(f64::INFINITY));
            let current_val = if current.is_int() {
                current.as_int().unwrap_or(i64::MAX) as f64
            } else {
                current.as_float().unwrap_or(f64::INFINITY)
            };
            let value_f64 = value as f64;
            if value_f64 < current_val {
                state.insert(key.to_string(), Dynamic::from(value));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "min");
        }
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_min", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_max overloads for different number types
    engine.register_fn("track_max", |key: &str, value: i64| {
        let updated = with_user_tracking(|state| {
            let current = state
                .get(key)
                .cloned()
                .unwrap_or(Dynamic::from(f64::NEG_INFINITY));
            let current_val = if current.is_int() {
                current.as_int().unwrap_or(i64::MIN) as f64
            } else {
                current.as_float().unwrap_or(f64::NEG_INFINITY)
            };
            let value_f64 = value as f64;
            if value_f64 > current_val {
                state.insert(key.to_string(), Dynamic::from(value));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "max");
        }
    });

    engine.register_fn("track_max", |key: &str, value: i32| {
        let updated = with_user_tracking(|state| {
            let current = state
                .get(key)
                .cloned()
                .unwrap_or(Dynamic::from(f64::NEG_INFINITY));
            let current_val = if current.is_int() {
                current.as_int().unwrap_or(i64::MIN) as f64
            } else {
                current.as_float().unwrap_or(f64::NEG_INFINITY)
            };
            let value_f64 = value as f64;
            if value_f64 > current_val {
                state.insert(key.to_string(), Dynamic::from(value));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "max");
        }
    });

    engine.register_fn("track_max", |key: &str, value: f64| {
        let updated = with_user_tracking(|state| {
            let current = state
                .get(key)
                .cloned()
                .unwrap_or(Dynamic::from(f64::NEG_INFINITY));
            let current_val = if current.is_int() {
                current.as_int().unwrap_or(i64::MIN) as f64
            } else {
                current.as_float().unwrap_or(f64::NEG_INFINITY)
            };
            if value > current_val {
                state.insert(key.to_string(), Dynamic::from(value));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "max");
        }
    });

    engine.register_fn("track_max", |key: &str, value: f32| {
        let updated = with_user_tracking(|state| {
            let current = state
                .get(key)
                .cloned()
                .unwrap_or(Dynamic::from(f64::NEG_INFINITY));
            let current_val = if current.is_int() {
                current.as_int().unwrap_or(i64::MIN) as f64
            } else {
                current.as_float().unwrap_or(f64::NEG_INFINITY)
            };
            let value_f64 = value as f64;
            if value_f64 > current_val {
                state.insert(key.to_string(), Dynamic::from(value));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "max");
        }
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_max", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_percentiles - streaming percentile estimation using t-digest
    // Auto-suffixes metric names with percentile (e.g., "latency_p95", "latency_p99")
    // This is the ONLY track_* function that auto-suffixes because percentiles are always multi-valued

    engine.register_fn(
        "track_percentiles",
        |key: &str, value: i64, percentiles: rhai::Array| {
            track_percentiles_impl(key, value as f64, percentiles)
        },
    );

    engine.register_fn(
        "track_percentiles",
        |key: &str, value: i32, percentiles: rhai::Array| {
            track_percentiles_impl(key, value as f64, percentiles)
        },
    );

    engine.register_fn(
        "track_percentiles",
        |key: &str, value: f64, percentiles: rhai::Array| {
            track_percentiles_impl(key, value, percentiles)
        },
    );

    engine.register_fn(
        "track_percentiles",
        |key: &str, value: f32, percentiles: rhai::Array| {
            track_percentiles_impl(key, value as f64, percentiles)
        },
    );

    // Unit overload - no-op for missing/empty values
    engine.register_fn(
        "track_percentiles",
        |_key: &str,
         _value: (),
         _percentiles: rhai::Array|
         -> Result<(), Box<rhai::EvalAltResult>> {
            // Silently ignore Unit values - no tracking occurs
            Ok(())
        },
    );

    engine.register_fn("track_unique", |key: &str, value: &str| {
        let updated = with_user_tracking(|state| {
            // Get existing set or create new one
            let current = state.get(key).cloned().unwrap_or_else(|| {
                // Create a new array to store unique values
                Dynamic::from(rhai::Array::new())
            });

            if let Ok(mut arr) = current.into_array() {
                let value_dynamic = Dynamic::from(value.to_string());
                // Check if value already exists in array
                if !arr
                    .iter()
                    .any(|v| v.clone().into_string().unwrap_or_default() == value)
                {
                    arr.push(value_dynamic);
                }
                state.insert(key.to_string(), Dynamic::from(arr));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "unique");
        }
    });

    engine.register_fn("track_unique", |key: &str, value: i64| {
        let updated = with_user_tracking(|state| {
            // Get existing set or create new one
            let current = state.get(key).cloned().unwrap_or_else(|| {
                // Create a new array to store unique values
                Dynamic::from(rhai::Array::new())
            });

            if let Ok(mut arr) = current.into_array() {
                let value_dynamic = Dynamic::from(value);
                // Check if value already exists in array
                if !arr.iter().any(|v| v.as_int().unwrap_or(i64::MIN) == value) {
                    arr.push(value_dynamic);
                }
                state.insert(key.to_string(), Dynamic::from(arr));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "unique");
        }
    });

    engine.register_fn("track_unique", |key: &str, value: i32| {
        let updated = with_user_tracking(|state| {
            // Get existing set or create new one
            let current = state.get(key).cloned().unwrap_or_else(|| {
                // Create a new array to store unique values
                Dynamic::from(rhai::Array::new())
            });

            if let Ok(mut arr) = current.into_array() {
                let value_dynamic = Dynamic::from(value);
                // Check if value already exists in array
                if !arr
                    .iter()
                    .any(|v| v.as_int().unwrap_or(i64::MIN) == (value as i64))
                {
                    arr.push(value_dynamic);
                }
                state.insert(key.to_string(), Dynamic::from(arr));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "unique");
        }
    });

    engine.register_fn("track_unique", |key: &str, value: f64| {
        let updated = with_user_tracking(|state| {
            // Get existing set or create new one
            let current = state.get(key).cloned().unwrap_or_else(|| {
                // Create a new array to store unique values
                Dynamic::from(rhai::Array::new())
            });

            if let Ok(mut arr) = current.into_array() {
                let value_dynamic = Dynamic::from(value);
                // Check if value already exists in array
                if !arr
                    .iter()
                    .any(|v| v.as_float().unwrap_or(f64::NAN) == value)
                {
                    arr.push(value_dynamic);
                }
                state.insert(key.to_string(), Dynamic::from(arr));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "unique");
        }
    });

    engine.register_fn("track_unique", |key: &str, value: f32| {
        let updated = with_user_tracking(|state| {
            // Get existing set or create new one
            let current = state.get(key).cloned().unwrap_or_else(|| {
                // Create a new array to store unique values
                Dynamic::from(rhai::Array::new())
            });

            if let Ok(mut arr) = current.into_array() {
                let value_dynamic = Dynamic::from(value);
                // Check if value already exists in array
                if !arr
                    .iter()
                    .any(|v| v.as_float().unwrap_or(f64::NAN) == (value as f64))
                {
                    arr.push(value_dynamic);
                }
                state.insert(key.to_string(), Dynamic::from(arr));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "unique");
        }
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_unique", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    engine.register_fn("track_bucket", |key: &str, bucket: &str| {
        let updated = with_user_tracking(|state| {
            // Get existing map or create new one
            let current = state
                .get(key)
                .cloned()
                .unwrap_or_else(|| Dynamic::from(rhai::Map::new()));

            if let Some(mut map) = current.try_cast::<rhai::Map>() {
                let count = map.get(bucket).cloned().unwrap_or(Dynamic::from(0i64));
                let new_count = count.as_int().unwrap_or(0) + 1;
                map.insert(bucket.into(), Dynamic::from(new_count));
                state.insert(key.to_string(), Dynamic::from(map));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "bucket");
        }
    });

    engine.register_fn("track_bucket", |key: &str, bucket: i64| {
        let updated = with_user_tracking(|state| {
            // Get existing map or create new one
            let current = state
                .get(key)
                .cloned()
                .unwrap_or_else(|| Dynamic::from(rhai::Map::new()));

            if let Some(mut map) = current.try_cast::<rhai::Map>() {
                let bucket_str = bucket.to_string();
                let count = map
                    .get(bucket_str.as_str())
                    .cloned()
                    .unwrap_or(Dynamic::from(0i64));
                let new_count = count.as_int().unwrap_or(0) + 1;
                map.insert(bucket_str.into(), Dynamic::from(new_count));
                state.insert(key.to_string(), Dynamic::from(map));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "bucket");
        }
    });

    engine.register_fn("track_bucket", |key: &str, bucket: i32| {
        let updated = with_user_tracking(|state| {
            // Get existing map or create new one
            let current = state
                .get(key)
                .cloned()
                .unwrap_or_else(|| Dynamic::from(rhai::Map::new()));

            if let Some(mut map) = current.try_cast::<rhai::Map>() {
                let bucket_str = bucket.to_string();
                let count = map
                    .get(bucket_str.as_str())
                    .cloned()
                    .unwrap_or(Dynamic::from(0i64));
                let new_count = count.as_int().unwrap_or(0) + 1;
                map.insert(bucket_str.into(), Dynamic::from(new_count));
                state.insert(key.to_string(), Dynamic::from(map));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "bucket");
        }
    });

    engine.register_fn("track_bucket", |key: &str, bucket: f64| {
        let updated = with_user_tracking(|state| {
            // Get existing map or create new one
            let current = state
                .get(key)
                .cloned()
                .unwrap_or_else(|| Dynamic::from(rhai::Map::new()));

            if let Some(mut map) = current.try_cast::<rhai::Map>() {
                let bucket_str = bucket.to_string();
                let count = map
                    .get(bucket_str.as_str())
                    .cloned()
                    .unwrap_or(Dynamic::from(0i64));
                let new_count = count.as_int().unwrap_or(0) + 1;
                map.insert(bucket_str.into(), Dynamic::from(new_count));
                state.insert(key.to_string(), Dynamic::from(map));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "bucket");
        }
    });

    engine.register_fn("track_bucket", |key: &str, bucket: f32| {
        let updated = with_user_tracking(|state| {
            // Get existing map or create new one
            let current = state
                .get(key)
                .cloned()
                .unwrap_or_else(|| Dynamic::from(rhai::Map::new()));

            if let Some(mut map) = current.try_cast::<rhai::Map>() {
                let bucket_str = bucket.to_string();
                let count = map
                    .get(bucket_str.as_str())
                    .cloned()
                    .unwrap_or(Dynamic::from(0i64));
                let new_count = count.as_int().unwrap_or(0) + 1;
                map.insert(bucket_str.into(), Dynamic::from(new_count));
                state.insert(key.to_string(), Dynamic::from(map));
                true
            } else {
                false
            }
        });
        if updated {
            record_operation_metadata(key, "bucket");
        }
    });

    // Unit overload - no-op for missing/empty values
    engine.register_fn("track_bucket", |_key: &str, _value: ()| {
        // Silently ignore Unit values - no tracking occurs
    });

    // track_top - Count mode: Most frequent items
    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }

            let updated = with_user_tracking(|state| {
                // Get existing array or create new one
                let current = state
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

                if let Ok(mut arr) = current.into_array() {
                    // Find existing item in array
                    let mut found_idx = None;
                    for (idx, elem) in arr.iter().enumerate() {
                        if let Some(map) = elem.clone().try_cast::<rhai::Map>() {
                            if let Some(k) = map.get("key") {
                                if k.clone().into_string().unwrap_or_default() == item_key {
                                    found_idx = Some(idx);
                                    break;
                                }
                            }
                        }
                    }

                    // Update count or add new item
                    if let Some(idx) = found_idx {
                        // Increment count for existing item
                        if let Some(map) = arr[idx].clone().try_cast::<rhai::Map>() {
                            let count = map.get("count").cloned().unwrap_or(Dynamic::from(0i64));
                            let new_count = count.as_int().unwrap_or(0) + 1;
                            let mut new_map = rhai::Map::new();
                            new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                            new_map.insert("count".into(), Dynamic::from(new_count));
                            arr[idx] = Dynamic::from(new_map);
                        }
                    } else {
                        // Add new item with count=1
                        let mut new_map = rhai::Map::new();
                        new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                        new_map.insert("count".into(), Dynamic::from(1i64));
                        arr.push(Dynamic::from(new_map));
                    }

                    // Sort by count descending, then by key ascending (stable sort)
                    arr.sort_by(|a, b| {
                        let a_map = a.clone().try_cast::<rhai::Map>();
                        let b_map = b.clone().try_cast::<rhai::Map>();

                        if let (Some(a_m), Some(b_m)) = (a_map, b_map) {
                            let a_count =
                                a_m.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                            let b_count =
                                b_m.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                            let a_key = a_m
                                .get("key")
                                .and_then(|v| v.clone().into_string().ok())
                                .unwrap_or_default();
                            let b_key = b_m
                                .get("key")
                                .and_then(|v| v.clone().into_string().ok())
                                .unwrap_or_default();

                            // Sort by count descending, then key ascending
                            match b_count.cmp(&a_count) {
                                std::cmp::Ordering::Equal => a_key.cmp(&b_key),
                                other => other,
                            }
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    });

                    // Trim to top N
                    if arr.len() > n as usize {
                        arr.truncate(n as usize);
                    }

                    state.insert(key.to_string(), Dynamic::from(arr));
                    true
                } else {
                    false
                }
            });

            if updated {
                record_operation_metadata(key, "top");
            }
            Ok(())
        },
    );

    // track_top - Weighted mode: Highest values
    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64, value: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }

            track_top_weighted_impl(key, item_key, n, value as f64)
        },
    );

    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64, value: i32| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }

            track_top_weighted_impl(key, item_key, n, value as f64)
        },
    );

    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64, value: f64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }

            track_top_weighted_impl(key, item_key, n, value)
        },
    );

    engine.register_fn(
        "track_top",
        |key: &str, item_key: &str, n: i64, value: f32| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_top requires n >= 1, got {}", n).into());
            }

            track_top_weighted_impl(key, item_key, n, value as f64)
        },
    );

    // Unit overload - no-op for missing/empty values
    engine.register_fn(
        "track_top",
        |_key: &str,
         _item_key: &str,
         _n: i64,
         _value: ()|
         -> Result<(), Box<rhai::EvalAltResult>> { Ok(()) },
    );

    // track_bottom - Count mode: Least frequent items
    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }

            let updated = with_user_tracking(|state| {
                // Get existing array or create new one
                let current = state
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

                if let Ok(mut arr) = current.into_array() {
                    // Find existing item in array
                    let mut found_idx = None;
                    for (idx, elem) in arr.iter().enumerate() {
                        if let Some(map) = elem.clone().try_cast::<rhai::Map>() {
                            if let Some(k) = map.get("key") {
                                if k.clone().into_string().unwrap_or_default() == item_key {
                                    found_idx = Some(idx);
                                    break;
                                }
                            }
                        }
                    }

                    // Update count or add new item
                    if let Some(idx) = found_idx {
                        // Increment count for existing item
                        if let Some(map) = arr[idx].clone().try_cast::<rhai::Map>() {
                            let count = map.get("count").cloned().unwrap_or(Dynamic::from(0i64));
                            let new_count = count.as_int().unwrap_or(0) + 1;
                            let mut new_map = rhai::Map::new();
                            new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                            new_map.insert("count".into(), Dynamic::from(new_count));
                            arr[idx] = Dynamic::from(new_map);
                        }
                    } else {
                        // Add new item with count=1
                        let mut new_map = rhai::Map::new();
                        new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                        new_map.insert("count".into(), Dynamic::from(1i64));
                        arr.push(Dynamic::from(new_map));
                    }

                    // Sort by count ascending, then by key ascending (stable sort)
                    arr.sort_by(|a, b| {
                        let a_map = a.clone().try_cast::<rhai::Map>();
                        let b_map = b.clone().try_cast::<rhai::Map>();

                        if let (Some(a_m), Some(b_m)) = (a_map, b_map) {
                            let a_count =
                                a_m.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                            let b_count =
                                b_m.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);
                            let a_key = a_m
                                .get("key")
                                .and_then(|v| v.clone().into_string().ok())
                                .unwrap_or_default();
                            let b_key = b_m
                                .get("key")
                                .and_then(|v| v.clone().into_string().ok())
                                .unwrap_or_default();

                            // Sort by count ascending, then key ascending
                            match a_count.cmp(&b_count) {
                                std::cmp::Ordering::Equal => a_key.cmp(&b_key),
                                other => other,
                            }
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    });

                    // Trim to bottom N
                    if arr.len() > n as usize {
                        arr.truncate(n as usize);
                    }

                    state.insert(key.to_string(), Dynamic::from(arr));
                    true
                } else {
                    false
                }
            });

            if updated {
                record_operation_metadata(key, "bottom");
            }
            Ok(())
        },
    );

    // track_bottom - Weighted mode: Lowest values
    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64, value: i64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }

            track_bottom_weighted_impl(key, item_key, n, value as f64)
        },
    );

    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64, value: i32| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }

            track_bottom_weighted_impl(key, item_key, n, value as f64)
        },
    );

    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64, value: f64| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }

            track_bottom_weighted_impl(key, item_key, n, value)
        },
    );

    engine.register_fn(
        "track_bottom",
        |key: &str, item_key: &str, n: i64, value: f32| -> Result<(), Box<rhai::EvalAltResult>> {
            if n < 1 {
                return Err(format!("track_bottom requires n >= 1, got {}", n).into());
            }

            track_bottom_weighted_impl(key, item_key, n, value as f64)
        },
    );

    // Unit overload - no-op for missing/empty values
    engine.register_fn(
        "track_bottom",
        |_key: &str,
         _item_key: &str,
         _n: i64,
         _value: ()|
         -> Result<(), Box<rhai::EvalAltResult>> { Ok(()) },
    );
}

/// Helper function for track_top weighted mode
fn track_top_weighted_impl(
    key: &str,
    item_key: &str,
    n: i64,
    value: f64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    let updated = with_user_tracking(|state| {
        // Get existing array or create new one
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current.into_array() {
            // Find existing item in array
            let mut found_idx = None;
            for (idx, elem) in arr.iter().enumerate() {
                if let Some(map) = elem.clone().try_cast::<rhai::Map>() {
                    if let Some(k) = map.get("key") {
                        if k.clone().into_string().unwrap_or_default() == item_key {
                            found_idx = Some(idx);
                            break;
                        }
                    }
                }
            }

            // Update value or add new item
            if let Some(idx) = found_idx {
                // Update value for existing item (take max)
                if let Some(map) = arr[idx].clone().try_cast::<rhai::Map>() {
                    let current_val = map
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::NEG_INFINITY);
                    let new_val = value.max(current_val);
                    let mut new_map = rhai::Map::new();
                    new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                    new_map.insert("value".into(), Dynamic::from(new_val));
                    arr[idx] = Dynamic::from(new_map);
                }
            } else {
                // Add new item
                let mut new_map = rhai::Map::new();
                new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                new_map.insert("value".into(), Dynamic::from(value));
                arr.push(Dynamic::from(new_map));
            }

            // Sort by value descending, then by key ascending (stable sort)
            arr.sort_by(|a, b| {
                let a_map = a.clone().try_cast::<rhai::Map>();
                let b_map = b.clone().try_cast::<rhai::Map>();

                if let (Some(a_m), Some(b_m)) = (a_map, b_map) {
                    let a_val = a_m
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::NEG_INFINITY);
                    let b_val = b_m
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::NEG_INFINITY);
                    let a_key = a_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();
                    let b_key = b_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();

                    // Sort by value descending, then key ascending
                    match b_val
                        .partial_cmp(&a_val)
                        .unwrap_or(std::cmp::Ordering::Equal)
                    {
                        std::cmp::Ordering::Equal => a_key.cmp(&b_key),
                        other => other,
                    }
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            // Trim to top N
            if arr.len() > n as usize {
                arr.truncate(n as usize);
            }

            state.insert(key.to_string(), Dynamic::from(arr));
            true
        } else {
            false
        }
    });

    if updated {
        record_operation_metadata(key, "top");
    }
    Ok(())
}

/// Helper function for track_bottom weighted mode
fn track_bottom_weighted_impl(
    key: &str,
    item_key: &str,
    n: i64,
    value: f64,
) -> Result<(), Box<rhai::EvalAltResult>> {
    let updated = with_user_tracking(|state| {
        // Get existing array or create new one
        let current = state
            .get(key)
            .cloned()
            .unwrap_or_else(|| Dynamic::from(rhai::Array::new()));

        if let Ok(mut arr) = current.into_array() {
            // Find existing item in array
            let mut found_idx = None;
            for (idx, elem) in arr.iter().enumerate() {
                if let Some(map) = elem.clone().try_cast::<rhai::Map>() {
                    if let Some(k) = map.get("key") {
                        if k.clone().into_string().unwrap_or_default() == item_key {
                            found_idx = Some(idx);
                            break;
                        }
                    }
                }
            }

            // Update value or add new item
            if let Some(idx) = found_idx {
                // Update value for existing item (take min)
                if let Some(map) = arr[idx].clone().try_cast::<rhai::Map>() {
                    let current_val = map
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::INFINITY);
                    let new_val = value.min(current_val);
                    let mut new_map = rhai::Map::new();
                    new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                    new_map.insert("value".into(), Dynamic::from(new_val));
                    arr[idx] = Dynamic::from(new_map);
                }
            } else {
                // Add new item
                let mut new_map = rhai::Map::new();
                new_map.insert("key".into(), Dynamic::from(item_key.to_string()));
                new_map.insert("value".into(), Dynamic::from(value));
                arr.push(Dynamic::from(new_map));
            }

            // Sort by value ascending, then by key ascending (stable sort)
            arr.sort_by(|a, b| {
                let a_map = a.clone().try_cast::<rhai::Map>();
                let b_map = b.clone().try_cast::<rhai::Map>();

                if let (Some(a_m), Some(b_m)) = (a_map, b_map) {
                    let a_val = a_m
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::INFINITY);
                    let b_val = b_m
                        .get("value")
                        .and_then(|v| v.as_float().ok())
                        .unwrap_or(f64::INFINITY);
                    let a_key = a_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();
                    let b_key = b_m
                        .get("key")
                        .and_then(|v| v.clone().into_string().ok())
                        .unwrap_or_default();

                    // Sort by value ascending, then key ascending
                    match a_val
                        .partial_cmp(&b_val)
                        .unwrap_or(std::cmp::Ordering::Equal)
                    {
                        std::cmp::Ordering::Equal => a_key.cmp(&b_key),
                        other => other,
                    }
                } else {
                    std::cmp::Ordering::Equal
                }
            });

            // Trim to bottom N
            if arr.len() > n as usize {
                arr.truncate(n as usize);
            }

            state.insert(key.to_string(), Dynamic::from(arr));
            true
        } else {
            false
        }
    });

    if updated {
        record_operation_metadata(key, "bottom");
    }
    Ok(())
}

/// Merge thread-local tracking state into context tracker for sequential mode
pub fn merge_thread_tracking_to_context(ctx: &mut crate::pipeline::PipelineContext) {
    let snapshot = get_thread_snapshot();
    for (key, value) in snapshot.user {
        ctx.tracker.insert(key, value);
    }
    for (key, value) in snapshot.internal {
        ctx.internal_tracker.insert(key, value);
    }
}

/// Format metrics for CLI output according to specification
pub fn format_metrics_output(metrics: &HashMap<String, Dynamic>, metrics_level: u8) -> String {
    let mut output = String::new();

    // Filter out internal keys (operation metadata and stats)
    let mut user_values: Vec<_> = metrics
        .iter()
        .filter(|(k, _)| !k.starts_with("__op_") && !k.starts_with("__kelora_stats_"))
        .collect();

    if user_values.is_empty() {
        return "No metrics tracked".to_string();
    }

    // Sort by key for consistent output
    user_values.sort_by_key(|(k, _)| k.as_str());

    for (key, value) in user_values {
        // Handle arrays (from track_unique, track_top, track_bottom) with smart truncation
        if value.is::<rhai::Array>() {
            if let Ok(arr) = value.clone().into_array() {
                let len = arr.len();

                // Check if this is a top/bottom array (array of maps with {key, count} or {key, value})
                let is_top_bottom = if !arr.is_empty() {
                    if let Some(first_map) = arr[0].clone().try_cast::<rhai::Map>() {
                        first_map.contains_key("key")
                            && (first_map.contains_key("count") || first_map.contains_key("value"))
                    } else {
                        false
                    }
                } else {
                    false
                };

                if is_top_bottom {
                    // Format track_top/track_bottom results
                    let field_name = if let Some(first_map) = arr[0].clone().try_cast::<rhai::Map>()
                    {
                        if first_map.contains_key("count") {
                            "count"
                        } else {
                            "value"
                        }
                    } else {
                        "count"
                    };

                    // Full output mode (--metrics=full): show everything
                    if metrics_level >= 2 {
                        output.push_str(&format!("{:<12} ({} items):\n", key, len));
                        for (idx, item) in arr.iter().enumerate() {
                            if let Some(map) = item.clone().try_cast::<rhai::Map>() {
                                if let (Some(k), Some(v)) = (map.get("key"), map.get(field_name)) {
                                    let key_str = k.clone().into_string().unwrap_or_default();
                                    if field_name == "count" {
                                        let count = v.as_int().unwrap_or(0);
                                        output.push_str(&format!(
                                            "  #{:<2} {:<30} {}\n",
                                            idx + 1,
                                            key_str,
                                            count
                                        ));
                                    } else {
                                        let val = v.as_float().unwrap_or(0.0);
                                        output.push_str(&format!(
                                            "  #{:<2} {:<30} {:.2}\n",
                                            idx + 1,
                                            key_str,
                                            val
                                        ));
                                    }
                                }
                            }
                        }
                    } else if len <= 10 {
                        // Small arrays: show all inline
                        output.push_str(&format!("{:<12} ({} items):\n", key, len));
                        for (idx, item) in arr.iter().enumerate() {
                            if let Some(map) = item.clone().try_cast::<rhai::Map>() {
                                if let (Some(k), Some(v)) = (map.get("key"), map.get(field_name)) {
                                    let key_str = k.clone().into_string().unwrap_or_default();
                                    if field_name == "count" {
                                        let count = v.as_int().unwrap_or(0);
                                        output.push_str(&format!(
                                            "  #{:<2} {:<30} {}\n",
                                            idx + 1,
                                            key_str,
                                            count
                                        ));
                                    } else {
                                        let val = v.as_float().unwrap_or(0.0);
                                        output.push_str(&format!(
                                            "  #{:<2} {:<30} {:.2}\n",
                                            idx + 1,
                                            key_str,
                                            val
                                        ));
                                    }
                                }
                            }
                        }
                    } else {
                        // Large arrays in abbreviated mode: show top 5 + hint
                        output.push_str(&format!("{:<12} ({} items):\n", key, len));
                        for (idx, item) in arr.iter().take(5).enumerate() {
                            if let Some(map) = item.clone().try_cast::<rhai::Map>() {
                                if let (Some(k), Some(v)) = (map.get("key"), map.get(field_name)) {
                                    let key_str = k.clone().into_string().unwrap_or_default();
                                    if field_name == "count" {
                                        let count = v.as_int().unwrap_or(0);
                                        output.push_str(&format!(
                                            "  #{:<2} {:<30} {}\n",
                                            idx + 1,
                                            key_str,
                                            count
                                        ));
                                    } else {
                                        let val = v.as_float().unwrap_or(0.0);
                                        output.push_str(&format!(
                                            "  #{:<2} {:<30} {:.2}\n",
                                            idx + 1,
                                            key_str,
                                            val
                                        ));
                                    }
                                }
                            }
                        }
                        output.push_str(&format!(
                            "  [+{} more. Use --metrics=full or --metrics-file for full list]\n",
                            len - 5
                        ));
                    }
                } else {
                    // Regular array (track_unique)
                    // Full output mode (--metrics=full): show everything
                    if metrics_level >= 2 {
                        output.push_str(&format!("{:<12} ({} unique):\n", key, len));
                        for item in arr.iter() {
                            output.push_str(&format!("  {}\n", item));
                        }
                    } else if len <= 10 {
                        // Small arrays: show inline
                        output.push_str(&format!("{:<12} = {}\n", key, value));
                    } else {
                        // Large arrays in abbreviated mode: show count + preview + hint
                        output.push_str(&format!("{:<12} ({} unique):\n", key, len));
                        for item in arr.iter().take(5) {
                            output.push_str(&format!("  {}\n", item));
                        }
                        output.push_str(&format!(
                            "  [+{} more. Use --metrics=full or --metrics-file for full list]\n",
                            len - 5
                        ));
                    }
                }
                continue;
            }
        }

        // Handle average tracking (map with sum and count)
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            if map.contains_key("sum") && map.contains_key("count") {
                let sum = map
                    .get("sum")
                    .and_then(|v| {
                        if v.is_float() {
                            v.as_float().ok()
                        } else if v.is_int() {
                            v.as_int().ok().map(|i| i as f64)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0.0);
                let count = map.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);

                let avg = if count > 0 { sum / count as f64 } else { 0.0 };
                output.push_str(&format!("{:<12} = {}\n", key, avg));
                continue;
            }
        }

        // Handle percentiles (t-digest blob)
        if let Ok(blob) = value.clone().into_blob() {
            // This is a t-digest - deserialize and compute the percentile
            if let Some(digest) = deserialize_tdigest(&blob) {
                // Extract percentile from key name (e.g., "api_latency_p95" -> 95.0)
                if let Some(p_pos) = key.rfind("_p") {
                    if let Ok(percentile) = key[p_pos + 2..].parse::<f64>() {
                        // Compute the percentile value
                        let quantile = percentile / 100.0;
                        let value = digest.estimate_quantile(quantile);
                        output.push_str(&format!("{:<12} = {:.2}\n", key, value));
                        continue;
                    }
                }
            }
            // If we can't deserialize or parse, fall through to default handling
        }

        // Handle regular values (count, max, etc.)
        if value.is_int() {
            output.push_str(&format!("{:<12} = {}\n", key, value.as_int().unwrap_or(0)));
        } else if value.is_float() {
            output.push_str(&format!(
                "{:<12} = {}\n",
                key,
                value.as_float().unwrap_or(0.0)
            ));
        } else {
            output.push_str(&format!("{:<12} = {}\n", key, value));
        }
    }

    output.trim_end().to_string()
}

fn dynamic_to_json(value: Dynamic) -> serde_json::Value {
    if value.is_unit() {
        return serde_json::Value::Null;
    }

    if value.is::<rhai::Array>() {
        if let Ok(array) = value.clone().into_array() {
            let json_array = array.into_iter().map(dynamic_to_json).collect();
            return serde_json::Value::Array(json_array);
        }
    }

    if value.is::<rhai::Map>() {
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            let mut json_map = serde_json::Map::new();
            for (k, v) in map {
                json_map.insert(k.into(), dynamic_to_json(v));
            }
            return serde_json::Value::Object(json_map);
        }
    }

    if value.is_int() {
        return serde_json::Value::Number(serde_json::Number::from(
            value.as_int().unwrap_or_default(),
        ));
    }

    if value.is_float() {
        if let Some(num) = serde_json::Number::from_f64(value.as_float().unwrap_or_default()) {
            return serde_json::Value::Number(num);
        }
    }

    if let Some(boolean) = value.clone().try_cast::<bool>() {
        return serde_json::Value::Bool(boolean);
    }

    if let Some(string) = value.clone().try_cast::<rhai::ImmutableString>() {
        return serde_json::Value::String(string.into());
    }

    serde_json::Value::String(value.to_string())
}

/// Format metrics for JSON output
pub fn format_metrics_json(
    metrics: &HashMap<String, Dynamic>,
) -> Result<String, serde_json::Error> {
    let mut json_obj = serde_json::Map::new();

    // Filter out internal keys
    for (key, value) in metrics.iter() {
        if key.starts_with("__op_")
            || key.starts_with("__kelora_stats_")
            || key.starts_with("__kelora_error_")
        {
            continue;
        }

        // Handle track_avg: compute and output the average
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            if map.contains_key("sum") && map.contains_key("count") {
                let sum = map
                    .get("sum")
                    .and_then(|v| {
                        if v.is_float() {
                            v.as_float().ok()
                        } else if v.is_int() {
                            v.as_int().ok().map(|i| i as f64)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0.0);
                let count = map.get("count").and_then(|v| v.as_int().ok()).unwrap_or(0);

                let avg = if count > 0 { sum / count as f64 } else { 0.0 };
                if let Some(num) = serde_json::Number::from_f64(avg) {
                    json_obj.insert(key.clone(), serde_json::Value::Number(num));
                } else {
                    json_obj.insert(key.clone(), serde_json::Value::Null);
                }
                continue;
            }
        }

        // Handle percentiles (t-digest blob)
        if let Ok(blob) = value.clone().into_blob() {
            // This is a t-digest - deserialize and compute the percentile
            if let Some(digest) = deserialize_tdigest(&blob) {
                // Extract percentile from key name (e.g., "api_latency_p95" -> 95.0)
                if let Some(p_pos) = key.rfind("_p") {
                    if let Ok(percentile) = key[p_pos + 2..].parse::<f64>() {
                        // Compute the percentile value
                        let quantile = percentile / 100.0;
                        let percentile_value = digest.estimate_quantile(quantile);
                        if let Some(num) = serde_json::Number::from_f64(percentile_value) {
                            json_obj.insert(key.clone(), serde_json::Value::Number(num));
                        } else {
                            json_obj.insert(key.clone(), serde_json::Value::Null);
                        }
                        continue;
                    }
                }
            }
        }

        json_obj.insert(key.clone(), dynamic_to_json(value.clone()));
    }

    serde_json::to_string_pretty(&json_obj)
}

/// Extract error summary from tracking state
#[allow(dead_code)] // Retained for potential future CLI summary output
pub fn extract_error_summary(metrics: &HashMap<String, Dynamic>) -> Option<String> {
    let mut has_errors = false;
    let mut summary = serde_json::Map::new();

    // Collect error types and their counts
    let mut error_types: std::collections::HashSet<String> = std::collections::HashSet::new();
    for key in metrics.keys() {
        if let Some(suffix) = key.strip_prefix("__kelora_error_count_") {
            error_types.insert(suffix.to_string());
        }
    }

    for error_type in error_types {
        let count_key = format!("__kelora_error_count_{}", error_type);
        let examples_key = format!("__kelora_error_examples_{}", error_type);

        if let Some(count_value) = metrics.get(&count_key) {
            let count = count_value.as_int().unwrap_or(0);
            if count > 0 {
                has_errors = true;
                let mut error_obj = serde_json::Map::new();
                error_obj.insert(
                    "count".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(count)),
                );

                // Add examples if available
                if let Some(examples_value) = metrics.get(&examples_key) {
                    if let Ok(examples_array) = examples_value.clone().into_array() {
                        let examples: Vec<serde_json::Value> = examples_array
                            .iter()
                            .map(|v| {
                                serde_json::Value::String(
                                    v.clone().into_string().unwrap_or_default(),
                                )
                            })
                            .collect();
                        error_obj
                            .insert("examples".to_string(), serde_json::Value::Array(examples));
                    }
                }

                summary.insert(error_type, serde_json::Value::Object(error_obj));
            }
        }
    }

    if has_errors {
        Some(
            serde_json::to_string_pretty(&summary)
                .unwrap_or_else(|_| "Error serializing summary".to_string()),
        )
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Dynamic;

    // Helper to clear thread-local state between tests
    fn clear_tracking_state() {
        THREAD_TRACKING_STATE.with(|state| {
            let mut snapshot = state.borrow_mut();
            snapshot.user.clear();
            snapshot.internal.clear();
        });
    }

    #[test]
    fn test_merge_numeric_integers() {
        let result = merge_numeric(Some(Dynamic::from(5i64)), Dynamic::from(3i64));
        assert_eq!(result.as_int().unwrap(), 8);
    }

    #[test]
    fn test_merge_numeric_floats() {
        let result = merge_numeric(Some(Dynamic::from(5.5f64)), Dynamic::from(3.2f64));
        let value = result.as_float().unwrap();
        assert!((value - 8.7).abs() < 0.001);
    }

    #[test]
    fn test_merge_numeric_mixed_int_and_float() {
        let result = merge_numeric(Some(Dynamic::from(5i64)), Dynamic::from(3.5f64));
        let value = result.as_float().unwrap();
        assert!((value - 8.5).abs() < 0.001);
    }

    #[test]
    fn test_merge_numeric_no_existing() {
        let result = merge_numeric(None, Dynamic::from(42i64));
        assert_eq!(result.as_int().unwrap(), 42);
    }

    #[test]
    fn test_get_set_thread_tracking_state() {
        clear_tracking_state();

        let mut metrics = HashMap::new();
        metrics.insert("test_key".to_string(), Dynamic::from(123i64));

        set_thread_tracking_state(&metrics);

        let retrieved = get_thread_tracking_state();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved.get("test_key").unwrap().as_int().unwrap(), 123);

        clear_tracking_state();
    }

    #[test]
    fn test_get_set_thread_internal_state() {
        clear_tracking_state();

        let mut internal = HashMap::new();
        internal.insert("internal_key".to_string(), Dynamic::from(456i64));

        set_thread_internal_state(&internal);

        let retrieved = get_thread_internal_state();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(
            retrieved.get("internal_key").unwrap().as_int().unwrap(),
            456
        );

        clear_tracking_state();
    }

    #[test]
    fn test_with_user_tracking() {
        clear_tracking_state();

        with_user_tracking(|state| {
            state.insert("key1".to_string(), Dynamic::from(100i64));
            state.insert("key2".to_string(), Dynamic::from(200i64));
        });

        let retrieved = get_thread_tracking_state();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved.get("key1").unwrap().as_int().unwrap(), 100);
        assert_eq!(retrieved.get("key2").unwrap().as_int().unwrap(), 200);

        clear_tracking_state();
    }

    #[test]
    fn test_with_internal_tracking() {
        clear_tracking_state();

        with_internal_tracking(|state| {
            state.insert("__internal1".to_string(), Dynamic::from(999i64));
        });

        let retrieved = get_thread_internal_state();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved.get("__internal1").unwrap().as_int().unwrap(), 999);

        clear_tracking_state();
    }

    #[test]
    fn test_record_operation_metadata() {
        clear_tracking_state();

        record_operation_metadata("test_key", "count");

        let internal = get_thread_internal_state();
        assert!(internal.contains_key("__op_test_key"));
        assert_eq!(
            internal
                .get("__op_test_key")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "count"
        );

        clear_tracking_state();
    }

    #[test]
    fn test_tracking_snapshot_from_parts() {
        let mut user = HashMap::new();
        user.insert("user_key".to_string(), Dynamic::from(1i64));

        let mut internal = HashMap::new();
        internal.insert("internal_key".to_string(), Dynamic::from(2i64));

        let snapshot = TrackingSnapshot::from_parts(user.clone(), internal.clone());

        assert_eq!(snapshot.user.len(), 1);
        assert_eq!(snapshot.internal.len(), 1);
        assert_eq!(snapshot.user.get("user_key").unwrap().as_int().unwrap(), 1);
        assert_eq!(
            snapshot
                .internal
                .get("internal_key")
                .unwrap()
                .as_int()
                .unwrap(),
            2
        );
    }

    #[test]
    fn test_get_thread_snapshot() {
        clear_tracking_state();

        with_user_tracking(|state| {
            state.insert("user_data".to_string(), Dynamic::from(111i64));
        });

        with_internal_tracking(|state| {
            state.insert("internal_data".to_string(), Dynamic::from(222i64));
        });

        let snapshot = get_thread_snapshot();
        assert_eq!(snapshot.user.len(), 1);
        assert_eq!(snapshot.internal.len(), 1);
        assert_eq!(
            snapshot.user.get("user_data").unwrap().as_int().unwrap(),
            111
        );
        assert_eq!(
            snapshot
                .internal
                .get("internal_data")
                .unwrap()
                .as_int()
                .unwrap(),
            222
        );

        clear_tracking_state();
    }

    #[test]
    fn test_has_errors_in_tracking_no_errors() {
        let snapshot = TrackingSnapshot::default();
        assert!(!has_errors_in_tracking(&snapshot));
    }

    #[test]
    fn test_has_errors_in_tracking_with_errors() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(5i64),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        assert!(has_errors_in_tracking(&snapshot));
    }

    #[test]
    fn test_has_errors_in_tracking_zero_count() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(0i64),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        assert!(!has_errors_in_tracking(&snapshot));
    }

    #[test]
    fn test_format_metrics_output_empty() {
        let metrics = HashMap::new();
        let output = format_metrics_output(&metrics, 1);
        assert_eq!(output, "No metrics tracked");
    }

    #[test]
    fn test_format_metrics_output_simple_values() {
        let mut metrics = HashMap::new();
        metrics.insert("count".to_string(), Dynamic::from(42i64));
        metrics.insert("sum".to_string(), Dynamic::from(2.5f64));

        let output = format_metrics_output(&metrics, 1);
        assert!(output.contains("count"));
        assert!(output.contains("42"));
        assert!(output.contains("sum"));
        assert!(output.contains("2.5"));
    }

    #[test]
    fn test_format_metrics_output_filters_internal_keys() {
        let mut metrics = HashMap::new();
        metrics.insert("user_metric".to_string(), Dynamic::from(100i64));
        metrics.insert("__op_user_metric".to_string(), Dynamic::from("count"));
        metrics.insert("__kelora_stats_lines".to_string(), Dynamic::from(50i64));

        let output = format_metrics_output(&metrics, 1);
        assert!(output.contains("user_metric"));
        assert!(!output.contains("__op_"));
        assert!(!output.contains("__kelora_stats_"));
    }

    #[test]
    fn test_format_metrics_output_small_array() {
        let mut metrics = HashMap::new();
        let arr = vec![
            Dynamic::from("val1"),
            Dynamic::from("val2"),
            Dynamic::from("val3"),
        ];
        metrics.insert("unique_vals".to_string(), Dynamic::from(arr));

        let output = format_metrics_output(&metrics, 1);
        assert!(output.contains("unique_vals"));
        assert!(output.contains("val1"));
        assert!(output.contains("val2"));
        assert!(output.contains("val3"));
    }

    #[test]
    fn test_format_metrics_output_large_array_abbreviated() {
        let mut metrics = HashMap::new();
        let mut arr = Vec::new();
        for i in 0..20 {
            arr.push(Dynamic::from(format!("item{}", i)));
        }
        metrics.insert("large_array".to_string(), Dynamic::from(arr));

        let output = format_metrics_output(&metrics, 1); // metrics_level = 1 (abbreviated)
        assert!(output.contains("large_array"));
        assert!(output.contains("20 unique"));
        assert!(output.contains("item0"));
        assert!(output.contains("item4"));
        assert!(output.contains("[+15 more"));
    }

    #[test]
    fn test_format_metrics_output_large_array_full() {
        let mut metrics = HashMap::new();
        let mut arr = Vec::new();
        for i in 0..20 {
            arr.push(Dynamic::from(format!("item{}", i)));
        }
        metrics.insert("large_array".to_string(), Dynamic::from(arr));

        let output = format_metrics_output(&metrics, 2); // metrics_level = 2 (full)
        assert!(output.contains("large_array"));
        assert!(output.contains("20 unique"));
        assert!(output.contains("item0"));
        assert!(output.contains("item19"));
        assert!(!output.contains("[+15 more")); // Should show all items
    }

    #[test]
    fn test_format_metrics_json_simple() {
        let mut metrics = HashMap::new();
        metrics.insert("count".to_string(), Dynamic::from(42i64));

        let json = format_metrics_json(&metrics).unwrap();
        assert!(json.contains("\"count\""));
        assert!(json.contains("42"));
    }

    #[test]
    fn test_format_metrics_json_filters_internal() {
        let mut metrics = HashMap::new();
        metrics.insert("user_metric".to_string(), Dynamic::from(100i64));
        metrics.insert("__op_user_metric".to_string(), Dynamic::from("count"));
        metrics.insert("__kelora_stats_lines".to_string(), Dynamic::from(50i64));
        metrics.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(5i64),
        );

        let json = format_metrics_json(&metrics).unwrap();
        assert!(json.contains("\"user_metric\""));
        assert!(!json.contains("\"__op_"));
        assert!(!json.contains("\"__kelora_stats_"));
        assert!(!json.contains("\"__kelora_error_"));
    }

    #[test]
    fn test_format_metrics_json_array() {
        let mut metrics = HashMap::new();
        let arr = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
        ];
        metrics.insert("numbers".to_string(), Dynamic::from(arr));

        let json = format_metrics_json(&metrics).unwrap();
        assert!(json.contains("\"numbers\""));
        assert!(json.contains("["));
        assert!(json.contains("1"));
        assert!(json.contains("2"));
        assert!(json.contains("3"));
    }

    #[test]
    fn test_format_metrics_json_map() {
        let mut metrics = HashMap::new();
        let mut map = rhai::Map::new();
        map.insert("key1".into(), Dynamic::from(10i64));
        map.insert("key2".into(), Dynamic::from(20i64));
        metrics.insert("buckets".to_string(), Dynamic::from(map));

        let json = format_metrics_json(&metrics).unwrap();
        assert!(json.contains("\"buckets\""));
        assert!(json.contains("\"key1\""));
        assert!(json.contains("10"));
        assert!(json.contains("\"key2\""));
        assert!(json.contains("20"));
    }

    #[test]
    fn test_dynamic_to_json_null() {
        let json = dynamic_to_json(Dynamic::UNIT);
        assert!(json.is_null());
    }

    #[test]
    fn test_dynamic_to_json_integer() {
        let json = dynamic_to_json(Dynamic::from(42i64));
        assert_eq!(json.as_i64().unwrap(), 42);
    }

    #[test]
    fn test_dynamic_to_json_float() {
        let json = dynamic_to_json(Dynamic::from(2.5f64));
        let val = json.as_f64().unwrap();
        assert!((val - 2.5).abs() < 0.001);
    }

    #[test]
    fn test_dynamic_to_json_string() {
        let json = dynamic_to_json(Dynamic::from("hello"));
        assert_eq!(json.as_str().unwrap(), "hello");
    }

    #[test]
    fn test_dynamic_to_json_bool() {
        let json = dynamic_to_json(Dynamic::from(true));
        assert!(json.as_bool().unwrap());
    }

    #[test]
    fn test_dynamic_to_json_array() {
        let arr = vec![
            Dynamic::from(1i64),
            Dynamic::from(2i64),
            Dynamic::from(3i64),
        ];
        let json = dynamic_to_json(Dynamic::from(arr));
        assert!(json.is_array());
        let array = json.as_array().unwrap();
        assert_eq!(array.len(), 3);
        assert_eq!(array[0].as_i64().unwrap(), 1);
        assert_eq!(array[1].as_i64().unwrap(), 2);
        assert_eq!(array[2].as_i64().unwrap(), 3);
    }

    #[test]
    fn test_dynamic_to_json_map() {
        let mut map = rhai::Map::new();
        map.insert("a".into(), Dynamic::from(100i64));
        map.insert("b".into(), Dynamic::from(200i64));
        let json = dynamic_to_json(Dynamic::from(map));
        assert!(json.is_object());
        let obj = json.as_object().unwrap();
        assert_eq!(obj.get("a").unwrap().as_i64().unwrap(), 100);
        assert_eq!(obj.get("b").unwrap().as_i64().unwrap(), 200);
    }

    #[test]
    fn test_format_error_location_single_file() {
        let input_files = vec!["test.log".to_string()];
        let location = format_error_location(Some(42), Some("test.log"), &input_files);
        assert_eq!(location, "line 42");
    }

    #[test]
    fn test_format_error_location_stdin() {
        let input_files: Vec<String> = vec![];
        let location = format_error_location(Some(10), None, &input_files);
        assert_eq!(location, "line 10");
    }

    #[test]
    fn test_format_error_location_multiple_files_no_conflict() {
        let input_files = vec!["file1.log".to_string(), "file2.log".to_string()];
        let location = format_error_location(Some(100), Some("file1.log"), &input_files);
        assert_eq!(location, "file1.log:100");
    }

    #[test]
    fn test_format_error_location_multiple_files_with_conflict() {
        let input_files = vec![
            "/path/to/file.log".to_string(),
            "/other/path/file.log".to_string(),
        ];
        let location = format_error_location(Some(50), Some("/path/to/file.log"), &input_files);
        assert_eq!(location, "/path/to/file.log:50");
    }

    #[test]
    fn test_format_error_location_no_line_number() {
        let input_files = vec!["test.log".to_string()];
        let location = format_error_location(None, Some("test.log"), &input_files);
        assert_eq!(location, "unknown");
    }

    #[test]
    fn test_extract_error_summary_no_errors() {
        let metrics = HashMap::new();
        let summary = extract_error_summary(&metrics);
        assert!(summary.is_none());
    }

    #[test]
    fn test_extract_error_summary_with_errors() {
        let mut metrics = HashMap::new();
        metrics.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(5i64),
        );

        let arr = vec![Dynamic::from("example error 1")];
        metrics.insert(
            "__kelora_error_examples_parse".to_string(),
            Dynamic::from(arr),
        );

        let summary = extract_error_summary(&metrics);
        assert!(summary.is_some());
        let text = summary.unwrap();
        assert!(text.contains("parse"));
        assert!(text.contains("\"count\": 5"));
    }

    #[test]
    fn test_extract_error_summary_zero_errors() {
        let mut metrics = HashMap::new();
        metrics.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(0i64),
        );

        let summary = extract_error_summary(&metrics);
        assert!(summary.is_none());
    }

    #[test]
    fn test_extract_error_summary_from_tracking_no_errors() {
        let snapshot = TrackingSnapshot::default();
        let summary = extract_error_summary_from_tracking(&snapshot, 0, None, None);
        assert!(summary.is_none());
    }

    #[test]
    fn test_extract_error_summary_from_tracking_with_errors() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(3i64),
        );

        // Create sample error objects
        let mut sample_obj = rhai::Map::new();
        sample_obj.insert("error_type".into(), Dynamic::from("parse"));
        sample_obj.insert("line_num".into(), Dynamic::from(42i64));
        sample_obj.insert("message".into(), Dynamic::from("Test error"));
        sample_obj.insert("filename".into(), Dynamic::from("test.log"));

        let samples = vec![Dynamic::from(sample_obj)];
        internal.insert(
            "__kelora_error_samples_parse".to_string(),
            Dynamic::from(samples),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);
        let summary = extract_error_summary_from_tracking(&snapshot, 0, None, None);

        assert!(summary.is_some());
        let text = summary.unwrap();
        assert!(text.contains("Parse errors: 3 total"));
        assert!(text.contains("Test error"));
    }

    #[test]
    fn test_extract_error_summary_adds_yearless_warning() {
        let mut internal = HashMap::new();
        internal.insert(
            "__kelora_error_count_parse".to_string(),
            Dynamic::from(2i64),
        );

        let snapshot = TrackingSnapshot::from_parts(HashMap::new(), internal);

        let stats = ProcessingStats {
            yearless_timestamps: 5,
            ..Default::default()
        };

        let summary =
            extract_error_summary_from_tracking(&snapshot, 0, Some(&stats), None).unwrap();

        assert!(summary.contains("Year-less timestamp format detected"));
        assert!(summary.contains("5 parse"));
    }

    #[test]
    fn test_merge_numeric_edge_case_zero_plus_zero() {
        let result = merge_numeric(Some(Dynamic::from(0i64)), Dynamic::from(0i64));
        assert_eq!(result.as_int().unwrap(), 0);
    }

    #[test]
    fn test_merge_numeric_edge_case_negative_numbers() {
        let result = merge_numeric(Some(Dynamic::from(-5i64)), Dynamic::from(-3i64));
        assert_eq!(result.as_int().unwrap(), -8);
    }

    #[test]
    fn test_merge_numeric_edge_case_large_integers() {
        let result = merge_numeric(
            Some(Dynamic::from(1_000_000_000i64)),
            Dynamic::from(2_000_000_000i64),
        );
        assert_eq!(result.as_int().unwrap(), 3_000_000_000i64);
    }

    #[test]
    fn test_thread_tracking_isolation() {
        clear_tracking_state();

        // Set initial state
        with_user_tracking(|state| {
            state.insert("test".to_string(), Dynamic::from(1i64));
        });

        // Verify state is set
        let state1 = get_thread_tracking_state();
        assert_eq!(state1.get("test").unwrap().as_int().unwrap(), 1);

        // Clear and verify empty
        clear_tracking_state();
        let state2 = get_thread_tracking_state();
        assert!(state2.is_empty());
    }

    #[test]
    fn test_track_top_count_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track some items
        engine
            .eval::<()>(r#"track_top("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "banana", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "cherry", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "apple", 3)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("test").unwrap().clone().into_array().unwrap();

        assert_eq!(result.len(), 3);

        // Check first item (apple: 3)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "apple"
        );
        assert_eq!(first.get("count").unwrap().as_int().unwrap(), 3);

        // Check second item (banana: 1 or cherry: 1, sorted alphabetically on tie)
        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(second.get("count").unwrap().as_int().unwrap(), 1);

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_n_limit() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track 5 items but limit to top 2
        engine
            .eval::<()>(r#"track_top("test", "item1", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item2", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item2", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item3", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item4", 2)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "item5", 2)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("test").unwrap().clone().into_array().unwrap();

        // Should only have top 2
        assert_eq!(result.len(), 2);

        // First should be item2 (count=2)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "item2"
        );
        assert_eq!(first.get("count").unwrap().as_int().unwrap(), 2);

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_weighted_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with custom values (latency)
        engine
            .eval::<()>(r#"track_top("slow", "/api/users", 2, 150)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("slow", "/api/products", 2, 50)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("slow", "/api/users", 2, 200)"#)
            .unwrap(); // Should update to max (200)
        engine
            .eval::<()>(r#"track_top("slow", "/api/orders", 2, 75)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("slow").unwrap().clone().into_array().unwrap();

        assert_eq!(result.len(), 2);

        // First should be /api/users with value=200 (max)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "/api/users"
        );
        assert_eq!(first.get("value").unwrap().as_float().unwrap(), 200.0);

        // Second should be /api/orders (75) or /api/products (50) - orders is higher
        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            second.get("key").unwrap().clone().into_string().unwrap(),
            "/api/orders"
        );
        assert_eq!(second.get("value").unwrap().as_float().unwrap(), 75.0);

        clear_tracking_state();
    }

    #[test]
    fn test_track_bottom_count_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track items with different frequencies
        // apple: 3 times, banana: 2 times, cherry: 1 time, date: 1 time
        engine
            .eval::<()>(r#"track_bottom("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "apple", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "banana", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "banana", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "cherry", 3)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("test", "date", 3)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("test").unwrap().clone().into_array().unwrap();

        // Should have bottom 3 (by count ascending, then alphabetically)
        assert_eq!(result.len(), 3);

        // First and second should both have count=1 (cherry and date, alphabetically)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(first.get("count").unwrap().as_int().unwrap(), 1);
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "cherry"
        );

        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(second.get("count").unwrap().as_int().unwrap(), 1);
        assert_eq!(
            second.get("key").unwrap().clone().into_string().unwrap(),
            "date"
        );

        // Third should be banana with count=2
        let third = result[2].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(third.get("count").unwrap().as_int().unwrap(), 2);
        assert_eq!(
            third.get("key").unwrap().clone().into_string().unwrap(),
            "banana"
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_bottom_weighted_mode() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with custom values (latency - bottom = fastest)
        engine
            .eval::<()>(r#"track_bottom("fast", "/api/users", 2, 150.5)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("fast", "/api/products", 2, 30.0)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_bottom("fast", "/api/users", 2, 100.0)"#)
            .unwrap(); // Should update to min (100)
        engine
            .eval::<()>(r#"track_bottom("fast", "/api/orders", 2, 75.0)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("fast").unwrap().clone().into_array().unwrap();

        assert_eq!(result.len(), 2);

        // First should be /api/products with value=30.0 (min)
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "/api/products"
        );
        assert_eq!(first.get("value").unwrap().as_float().unwrap(), 30.0);

        // Second should be /api/orders (75.0) or /api/users (100.0) - orders is smaller
        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            second.get("key").unwrap().clone().into_string().unwrap(),
            "/api/orders"
        );
        assert_eq!(second.get("value").unwrap().as_float().unwrap(), 75.0);

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_invalid_n() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // N must be >= 1
        let result = engine.eval::<()>(r#"track_top("test", "item", 0)"#);
        assert!(result.is_err());

        let result = engine.eval::<()>(r#"track_top("test", "item", -1)"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_track_top_unit_value() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Unit values should be silently ignored
        engine
            .eval::<()>(r#"track_top("test", "apple", 3, ())"#)
            .unwrap();

        let state = get_thread_tracking_state();
        // Should not have created any entry or should be empty
        assert!(
            !state.contains_key("test")
                || state
                    .get("test")
                    .unwrap()
                    .clone()
                    .into_array()
                    .unwrap()
                    .is_empty()
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_top_stable_sort() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Create ties - all have count=1
        engine
            .eval::<()>(r#"track_top("test", "zebra", 5)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "apple", 5)"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_top("test", "mango", 5)"#)
            .unwrap();

        let state = get_thread_tracking_state();
        let result = state.get("test").unwrap().clone().into_array().unwrap();

        // All have same count, so should be sorted alphabetically
        let first = result[0].clone().try_cast::<rhai::Map>().unwrap();
        let second = result[1].clone().try_cast::<rhai::Map>().unwrap();
        let third = result[2].clone().try_cast::<rhai::Map>().unwrap();

        assert_eq!(
            first.get("key").unwrap().clone().into_string().unwrap(),
            "apple"
        );
        assert_eq!(
            second.get("key").unwrap().clone().into_string().unwrap(),
            "mango"
        );
        assert_eq!(
            third.get("key").unwrap().clone().into_string().unwrap(),
            "zebra"
        );

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_basic() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track some values
        engine
            .eval::<()>(r#"track_percentiles("latency", 100, [50, 95, 99])"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_percentiles("latency", 200, [50, 95, 99])"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_percentiles("latency", 150, [50, 95, 99])"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_percentiles("latency", 300, [50, 95, 99])"#)
            .unwrap();
        engine
            .eval::<()>(r#"track_percentiles("latency", 250, [50, 95, 99])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // Check that all three percentile metrics were created
        assert!(state.contains_key("latency_p50"));
        assert!(state.contains_key("latency_p95"));
        assert!(state.contains_key("latency_p99"));

        // Check that they're blobs (serialized t-digest)
        assert!(state.get("latency_p50").unwrap().is_blob());
        assert!(state.get("latency_p95").unwrap().is_blob());
        assert!(state.get("latency_p99").unwrap().is_blob());

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_single() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track single percentile
        engine
            .eval::<()>(r#"track_percentiles("response_time", 123.45, [95])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        assert!(state.contains_key("response_time_p95"));
        assert!(state.get("response_time_p95").unwrap().is_blob());

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_dedup() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Track with duplicate percentiles
        engine
            .eval::<()>(r#"track_percentiles("test", 100, [95, 95, 99, 95])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // Should only create two metrics (deduped)
        assert!(state.contains_key("test_p95"));
        assert!(state.contains_key("test_p99"));

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_invalid_percentile() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Out of range percentile
        let result = engine.eval::<()>(r#"track_percentiles("test", 100, [101])"#);
        assert!(result.is_err());

        let result = engine.eval::<()>(r#"track_percentiles("test", 100, [-5])"#);
        assert!(result.is_err());

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_empty_array() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Empty percentiles array
        let result = engine.eval::<()>(r#"track_percentiles("test", 100, [])"#);
        assert!(result.is_err());

        clear_tracking_state();
    }

    #[test]
    fn test_track_percentiles_unit_value() {
        clear_tracking_state();

        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Unit values should be silently ignored
        engine
            .eval::<()>(r#"track_percentiles("test", (), [95])"#)
            .unwrap();

        let state = get_thread_tracking_state();

        // No metric should be created for unit value
        assert!(!state.contains_key("test_p95"));

        clear_tracking_state();
    }
}
