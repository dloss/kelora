use crate::config::InputFormat as ConfigInputFormat;
use crate::parsers::{CefParser, CombinedParser, LogfmtParser, SyslogParser};
use crate::pipeline::EventParser;
use anyhow::Result;

/// Auto-detect the input format based on the first line of input.
/// Tries formats in order of specificity/commonality with 'line' as fallback.
///
/// Format detection priority:
/// 1. JSON - starts with '{' and valid JSON
/// 2. CEF - starts with "CEF:"
/// 3. Syslog - matches RFC5424 or RFC3164 patterns
/// 4. Combined - contains common Apache/Nginx log patterns
/// 5. Logfmt - contains key=value pairs
/// 6. CSV/TSV - contains delimiters with reasonable structure, and (for the
///    with-header variants) a first line that reads as a header row
/// 7. Named application-log formats adapted from lnav (regex-based)
/// 8. Line - fallback for everything else
pub fn detect_format(sample_line: &str) -> Result<ConfigInputFormat> {
    let trimmed = sample_line.trim();

    // Empty line detection - default to line format
    if trimmed.is_empty() {
        return Ok(ConfigInputFormat::Line);
    }

    // 1. JSON detection - most specific
    if detect_json(trimmed) {
        return Ok(ConfigInputFormat::Json);
    }

    // 2. CEF detection - very specific prefix
    if detect_cef(trimmed) {
        return Ok(ConfigInputFormat::Cef);
    }

    // 3. Syslog detection - structured patterns
    if detect_syslog(trimmed) {
        return Ok(ConfigInputFormat::Syslog);
    }

    // 4. Combined log format detection (Apache/Nginx)
    if detect_combined_logs(trimmed) {
        return Ok(ConfigInputFormat::Combined);
    }

    // 5. Kubernetes CRI / containerd container log: `<RFC3339Nano> <stream> <F|P> msg`.
    //    This prefix is highly specific, but the message after it is frequently
    //    JSON or logfmt, so it must be claimed *before* the logfmt and CSV steps
    //    (a JSON message's commas would otherwise trip CSV; key=value pairs would
    //    trip logfmt). Unlike the other named formats — which are only tried as
    //    the last step before `line` — CRI gets a dedicated early detector so
    //    auto-detection works regardless of the message payload.
    if let Some(fmt) = detect_cri(trimmed) {
        return Ok(ConfigInputFormat::Named(fmt));
    }

    // 6. Logfmt detection - key=value patterns
    if detect_logfmt(trimmed) {
        return Ok(ConfigInputFormat::Logfmt);
    }

    // 7. CSV/TSV detection
    if let Some(csv_format) = detect_csv_variants(trimmed) {
        return Ok(csv_format);
    }

    // 8. Built-in named application-log formats adapted from lnav.
    //    Tried last (just before the line fallback) so it can only reclassify
    //    input that would otherwise become `line` — never a format already
    //    detected above. Returns the named format (regex-backed) so the notice
    //    and stats show its name (e.g. "log4j") rather than a bare "regex".
    if let Some(fmt) = crate::parsers::lnav_formats::detect(trimmed) {
        return Ok(ConfigInputFormat::Named(fmt));
    }

    // 9. Fallback to line format
    Ok(ConfigInputFormat::Line)
}

/// What a multi-line sample detected: the format to parse with, plus any
/// structured formats the sample contained that the format does *not* parse.
#[derive(Debug)]
pub struct SampleDetection {
    /// The format the run should use. One of: a single format (homogeneous
    /// sample, or whole-file csv/tsv), plain `line`, or a two-member
    /// `cascade(X,line)` — auto-detection never builds anything wider.
    pub format: ConfigInputFormat,
    /// Display names of structured formats seen in the sample but left out of
    /// `format` (rank order). Their lines will parse as whole `line` events;
    /// detection surfaces them via a hint suggesting `suggested_cascade`.
    pub unparsed_formats: Vec<String>,
    /// The explicit `-f` value that would parse everything the sample showed
    /// (all structured formats by specificity, `line` last). `Some` exactly
    /// when `unparsed_formats` is non-empty.
    pub suggested_cascade: Option<String>,
}

impl SampleDetection {
    fn plain(format: ConfigInputFormat) -> Self {
        Self {
            format,
            unparsed_formats: Vec::new(),
            suggested_cascade: None,
        }
    }
}

/// In samples of at least this many lines, a structured format needs
/// [`CASCADE_QUORUM`] matching lines to anchor a cascade. Below it, a single
/// line is a meaningful fraction of the file and counts on its own.
const CASCADE_QUORUM_MIN_SAMPLE: usize = 4;
/// Sampled lines a structured format must claim before auto-detection builds
/// a cascade around it. With head + probe samples reaching ~128 lines, a
/// single matching line is far more likely to be an accident (one
/// `key=value key2=value2`-shaped line in a CI log) than a format the file
/// really contains — and one accidental member used to change how every
/// other line in the file was parsed.
const CASCADE_QUORUM: usize = 2;

/// Detect the input format from a multi-line sample instead of a single line.
///
/// Each sampled line is classified with [`detect_format`]. A homogeneous sample
/// returns exactly what single-line detection would have returned, so files
/// that detect cleanly today are unaffected. A *mixed* sample — the messy
/// real-world file this exists for (JSON with plain-text startup lines, a
/// service log with interleaved tracebacks) — parses with a capped cascade:
///
/// - **At most one structured format joins.** The dominant one — most sampled
///   lines, ties broken by detection specificity — paired with the `line`
///   catch-all: `cascade(json,line)`, never `cascade(json,syslog,logfmt,…)`.
///   Detection is per-line parser acceptance, which has good recall but
///   imperfect precision; a wide auto-built cascade let any accidental member
///   claim lines it was never voted in by. Files that genuinely interleave
///   several structured formats keep working line-by-line (the extras parse
///   as `line`), and the returned `unparsed_formats`/`suggested_cascade`
///   drive a hint telling the user the explicit `-f json,syslog,line` that
///   parses everything.
/// - **Membership needs a quorum** ([`CASCADE_QUORUM`]): one matching line in
///   a large sample doesn't recruit a parser for the whole file.
/// - **`line` is always the second member**, even when no sampled line needed
///   it, so an auto-built cascade is total: a line past the sample window can
///   never become a parse error on the strength of a guess. (Explicit user
///   cascades are not padded this way — `-f json,logfmt` failing loudly on
///   other content is what the user asked for.)
///
/// Two rules keep the schema-based formats sane:
///
/// - If the *first* line reads as csv/tsv, that format is returned immediately.
///   A schema format owns the whole file (its header/column layout is fixed at
///   the head), can't participate in a cascade, and its data rows would
///   re-detect as assorted csv variants anyway.
/// - A csv/tsv detection on a *later* line is treated as `line`. A file that
///   doesn't start as CSV can't be parsed as CSV mid-stream, so such a line is
///   really an unstructured line with delimiter-shaped content (e.g. a log
///   message containing commas) — exactly what the cascade's `line` member is
///   for.
pub fn detect_format_from_sample<'a, I>(lines: I) -> Result<SampleDetection>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut iter = lines.into_iter();
    let Some(first) = iter.next() else {
        return Ok(SampleDetection::plain(ConfigInputFormat::Line));
    };

    let first_format = detect_format(first)?;
    if is_schema_format(&first_format) {
        return Ok(SampleDetection::plain(first_format));
    }

    let mut tallies: Vec<(ConfigInputFormat, usize)> = vec![(first_format, 1)];
    let mut total_lines = 1usize;
    for line in iter {
        total_lines += 1;
        let detected = detect_format(line)?;
        let fmt = if is_schema_format(&detected) {
            ConfigInputFormat::Line
        } else {
            detected
        };
        match tallies
            .iter_mut()
            .find(|(m, _)| m.to_display_string() == fmt.to_display_string())
        {
            Some(entry) => entry.1 += 1,
            None => tallies.push((fmt, 1)),
        }
    }

    if tallies.len() == 1 {
        return Ok(SampleDetection::plain(tallies.remove(0).0));
    }

    // Mixed sample: pick the dominant structured format. Stable sort, so
    // equal (count, rank) keys — e.g. two named app-log formats tied on
    // support — keep their first-seen order.
    let mut structured: Vec<(ConfigInputFormat, usize)> = tallies
        .into_iter()
        .filter(|(fmt, _)| !matches!(fmt, ConfigInputFormat::Line))
        .collect();
    structured.sort_by_key(|(fmt, count)| (std::cmp::Reverse(*count), cascade_rank(fmt)));

    let quorum = if total_lines >= CASCADE_QUORUM_MIN_SAMPLE {
        CASCADE_QUORUM
    } else {
        1
    };
    let dominant = (structured.first().map_or(0, |(_, count)| *count) >= quorum)
        .then(|| structured.remove(0).0);

    // Everything structured that we are not parsing — for the hint. The
    // suggestion includes the dominant format too, so it is a complete `-f`
    // value, ordered by specificity with the catch-all last.
    let mut unparsed: Vec<ConfigInputFormat> = structured.into_iter().map(|(fmt, _)| fmt).collect();
    unparsed.sort_by_key(cascade_rank);
    let suggested_cascade = (!unparsed.is_empty()).then(|| {
        let mut all: Vec<&ConfigInputFormat> = unparsed.iter().chain(dominant.iter()).collect();
        all.sort_by_key(|fmt| cascade_rank(fmt));
        let mut names: Vec<String> = all.iter().map(|fmt| fmt.to_display_string()).collect();
        names.push("line".to_string());
        names.join(",")
    });
    let unparsed_formats: Vec<String> = unparsed
        .iter()
        .map(ConfigInputFormat::to_display_string)
        .collect();

    let format = match dominant {
        Some(fmt) => ConfigInputFormat::Cascade(vec![fmt, ConfigInputFormat::Line]),
        None => ConfigInputFormat::Line,
    };
    Ok(SampleDetection {
        format,
        unparsed_formats,
        suggested_cascade,
    })
}

/// CSV/TSV in any variant: the formats whose schema is fixed at the head of
/// the file and which therefore can't join a per-line cascade.
fn is_schema_format(fmt: &ConfigInputFormat) -> bool {
    matches!(
        fmt,
        ConfigInputFormat::Csv(_)
            | ConfigInputFormat::Tsv(_)
            | ConfigInputFormat::Csvnh
            | ConfigInputFormat::Tsvnh
    )
}

/// Ordering for auto-built cascades, mirroring the priority order of
/// [`detect_format`]: more specific formats first, so they get first shot at
/// each line, with the catch-all `line` guaranteed last (as the cascade
/// validation rules require).
fn cascade_rank(fmt: &ConfigInputFormat) -> u8 {
    match fmt {
        ConfigInputFormat::Json => 0,
        ConfigInputFormat::Cef => 1,
        ConfigInputFormat::Syslog => 2,
        ConfigInputFormat::Combined => 3,
        // CRI is detected before logfmt (its message payload is often logfmt
        // or JSON), so it must also *parse* before logfmt in a cascade.
        ConfigInputFormat::Named(f) if f.name == "cri" => 4,
        ConfigInputFormat::Logfmt => 5,
        ConfigInputFormat::Named(_) => 6,
        ConfigInputFormat::Line => u8::MAX,
        // Unreachable from detection; rank just below the catch-all.
        _ => u8::MAX - 1,
    }
}

/// Detect JSON format - starts with '{' and is valid JSON
fn detect_json(line: &str) -> bool {
    if !line.starts_with('{') {
        return false;
    }

    // Try to parse as JSON - if it succeeds, it's likely JSON
    serde_json::from_str::<serde_json::Value>(line).is_ok()
}

/// Detection parsers are built once per process and shared by every classified
/// line.
///
/// Detection runs per *sampled* line — up to `SAMPLE_MAX_LINES` head lines plus
/// mid-file probe lines — and the syslog/combined parsers each compile several
/// regexes at construction. Rebuilding them per line turned a fixed startup cost
/// into a per-line one, which dominated the run on small files.
///
/// A parser whose regexes fail to compile is simply absent, which makes its
/// detector return `false` — the same outcome the previous per-call code had for
/// a compile error, and unreachable in practice for these static patterns.
fn cef_detector() -> &'static CefParser {
    static PARSER: std::sync::OnceLock<CefParser> = std::sync::OnceLock::new();
    // Strict mode for detection - we only want true positives.
    PARSER.get_or_init(|| CefParser::new_without_auto_timestamp().with_strict(true))
}

fn syslog_detector() -> Option<&'static SyslogParser> {
    static PARSER: std::sync::OnceLock<Option<SyslogParser>> = std::sync::OnceLock::new();
    PARSER
        .get_or_init(|| SyslogParser::new_without_auto_timestamp().ok())
        .as_ref()
}

fn combined_detector() -> Option<&'static CombinedParser> {
    static PARSER: std::sync::OnceLock<Option<CombinedParser>> = std::sync::OnceLock::new();
    PARSER
        .get_or_init(|| CombinedParser::new_without_auto_timestamp().ok())
        .as_ref()
}

fn logfmt_detector() -> &'static LogfmtParser {
    static PARSER: std::sync::OnceLock<LogfmtParser> = std::sync::OnceLock::new();
    PARSER.get_or_init(LogfmtParser::new_without_auto_timestamp)
}

/// Detect CEF format using actual parser for 100% accuracy
fn detect_cef(line: &str) -> bool {
    cef_detector().parse(line).is_ok()
}

/// Detect Syslog format using actual parser for 100% accuracy
fn detect_syslog(line: &str) -> bool {
    syslog_detector().is_some_and(|parser| parser.parse(line).is_ok())
}

/// Detect combined log formats (Apache/Nginx) using actual parser for 100% accuracy
fn detect_combined_logs(line: &str) -> bool {
    combined_detector().is_some_and(|parser| parser.parse(line).is_ok())
}

/// Detect the Kubernetes CRI / containerd container-log layout
/// (`<RFC3339Nano> <stream> <F|P> <message>`) by reusing the `cri` named
/// format's own pattern, so detection and `-f cri` share one source of truth.
/// Returns the static format definition so the auto-detect notice and `--stats`
/// show the name `cri` (rather than a bare `regex`).
fn detect_cri(line: &str) -> Option<&'static crate::parsers::lnav_formats::LnavFormat> {
    crate::parsers::lnav_formats::matches_named("cri", line)
}

/// Detect logfmt with stricter rules than the parser itself.
///
/// The parser is deliberately tolerant — under an explicit `-f logfmt` a line
/// is whatever the user says it is, and salvaging is right. Detection needs
/// precision instead: `parser.parse(line).is_ok()` accepts any line whose
/// every whitespace token contains `=`, which claims URLs with query strings
/// (`…/search?q=timeout&limit=10`), base64 padding (`…Zw==`), env/assignment
/// dumps (`PATH=/usr/bin`), and CLI flags (`--config=/etc/app.yaml`). Two
/// extra requirements kill that entire family while keeping real logfmt:
///
/// - **at least two key=value pairs** — every accidental match above is a
///   single "assignment-shaped" token, while genuine logfmt records carry
///   several fields (a pure `cache=hit`-style single-pair log would no longer
///   auto-detect, and wants an explicit `-f logfmt`);
/// - **no unterminated quotes** (`parse_strict`) — an unclosed quote
///   swallowing the rest of the line into one value is evidence *against*
///   logfmt, even though the parser tolerates it for truncated lines.
fn detect_logfmt(line: &str) -> bool {
    logfmt_detector()
        .parse_strict(line)
        .map(|event| event.fields.len() >= 2)
        .unwrap_or(false)
}

/// Detect CSV/TSV variants
fn detect_csv_variants(line: &str) -> Option<ConfigInputFormat> {
    let comma_count = line.matches(',').count();
    let tab_count = line.matches('\t').count();

    // Require multiple delimiters to distinguish from random commas/tabs in text
    if tab_count >= 2 {
        // Check if it could have headers vs no headers
        // If first field looks like a column name (letters), assume headers
        if let Some(first_field) = line.split('\t').next() {
            if first_field.chars().any(|c| c.is_ascii_alphabetic())
                && !first_field.chars().all(|c| c.is_ascii_digit())
            {
                if plausible_header_row(line, '\t') {
                    return Some(ConfigInputFormat::Tsv(None));
                }
            } else {
                return Some(ConfigInputFormat::Tsvnh);
            }
        }
    }

    if comma_count >= 2 {
        // Similar logic for CSV
        if let Some(first_field) = line.split(',').next() {
            let trimmed_field = first_field.trim_matches('"').trim();
            if trimmed_field.chars().any(|c| c.is_ascii_alphabetic())
                && !trimmed_field.chars().all(|c| c.is_ascii_digit())
            {
                if plausible_header_row(line, ',') {
                    return Some(ConfigInputFormat::Csv(None));
                }
            } else {
                return Some(ConfigInputFormat::Csvnh);
            }
        }
    }

    None
}

/// Could `line` be a CSV/TSV *header* row, or is it an application log line that
/// merely contains delimiters?
///
/// Only consulted when auto-detection is about to pick the with-header variant
/// (`csv`/`tsv`). That branch is the damaging one: the first line is consumed as
/// the header, so a log line there turns every field name into a fragment of a
/// log message and silently mislabels the whole stream (exit 0, no diagnostic).
/// Commas are common in log messages — JVM list rendering (`[TERM,HUP,INT]`),
/// `acls to: a,b`, comma-separated paths — so this fires often in practice.
///
/// Returning false makes detection fall through to the application-log formats
/// and finally `line`, which is the honest outcome and comes with the existing
/// "No input format detected" hint.
///
/// The headerless variants (`csvnh`/`tsvnh`) are deliberately not checked: their
/// field names are positional, so there is no header to corrupt, and a genuine
/// CSV export of log records (`2024-01-02 10:00:00,INFO,started`) must keep
/// detecting as `csvnh`. An explicit `-f csv` never reaches here either — that
/// stays maximally permissive.
fn plausible_header_row(line: &str, delimiter: char) -> bool {
    line.split(delimiter)
        .map(|field| field.trim().trim_matches('"').trim())
        .all(plausible_header_name)
}

/// Reject header names that carry the fingerprints of a log line.
///
/// The discriminator is *company*, not vocabulary. A log line carries its level
/// and timestamp alongside the rest of the message in the same field
/// (`17/06/09 20:10:40 INFO executor.Backend: …`), whereas a header field that
/// happens to be a level or a date is that field's entire contents. So a bare
/// `INFO` or `2024-01-01` only counts as evidence when the field holds other
/// words too — which keeps two shapes of real CSV working that an
/// anywhere-in-the-field test would wrongly reject:
///
/// - all-caps SQL/warehouse exports: `TIMESTAMP,ERROR_COUNT,WARN_COUNT`
/// - wide time-series columns:       `region,2024-01-01,2024-01-02`
///
/// Vocabulary is never consulted: `timestamp,level,message` is fine, because
/// these test for digit-shaped values and upper-case severity words.
fn plausible_header_name(field: &str) -> bool {
    if is_prose_like(field) {
        return false;
    }
    // A full date+time in one token is a timestamp wherever it appears — no
    // column is named `2024-01-02T10:00:00Z`.
    if field.split_whitespace().any(is_iso_datetime) {
        return false;
    }
    let multi_word = field.split_whitespace().count() >= 2;
    !(multi_word && (contains_level_token(field) || contains_timestamp_token(field)))
}

/// A bare, standalone severity word — the classic middle column of an app log
/// line. Tokenised on *whitespace* (then stripped of edge punctuation, so
/// `[INFO]` and `INFO:` still match), which is what keeps `ERROR_COUNT` and
/// `info.hits` from reading as levels. Upper case only, so a header named
/// `error` or `Warnings` is untouched.
fn contains_level_token(field: &str) -> bool {
    const LEVELS: &[&str] = &[
        "TRACE", "DEBUG", "INFO", "NOTICE", "WARN", "WARNING", "ERROR", "SEVERE", "CRIT",
        "CRITICAL", "FATAL", "PANIC", "ALERT", "EMERG",
    ];
    field
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
        .any(|token| LEVELS.contains(&token))
}

/// A date- or clock-shaped token: `2024-01-02`, `17/06/09`, `01/02/2024`,
/// `2024.01.02`, or `20:10:40`. Digit-shaped, so `date`/`time`/`created_at`
/// header names don't trip it.
fn contains_timestamp_token(field: &str) -> bool {
    field
        .split_whitespace()
        .any(|token| looks_like_date(token) || looks_like_clock(token))
}

/// A single token holding both halves of a timestamp, ISO-style:
/// `2024-01-02T10:00:00Z`, `2024-01-02T10:00:00.123+02:00`. Only the `HH:MM`
/// prefix of the time part is checked, so any trailing zone spelling is
/// tolerated without trying to parse it.
fn is_iso_datetime(token: &str) -> bool {
    let Some((date, time)) = token.split_once('T') else {
        return false;
    };
    if !looks_like_date(date) {
        return false;
    }
    matches!(time.as_bytes(),
        [h1, h2, b':', m1, m2, ..]
            if h1.is_ascii_digit() && h2.is_ascii_digit()
                && m1.is_ascii_digit() && m2.is_ascii_digit())
}

/// `N+SEP N+SEP N+` where SEP is `-`, `/`, or `.` — the separator repeated, so a
/// lone `2024-01` or a version like `1.2` doesn't match.
fn looks_like_date(token: &str) -> bool {
    ['-', '/', '.'].iter().any(|&sep| {
        let parts: Vec<&str> = token.trim_end_matches([',', ';', ':']).split(sep).collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    })
}

/// `HH:MM:SS`, optionally with fractional seconds (`20:10:40.123`).
fn looks_like_clock(token: &str) -> bool {
    let parts: Vec<&str> = token.trim_end_matches([',', ';']).split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    let (last, head) = parts.split_last().expect("checked len == 3");
    let seconds = last.split_once('.').map_or(*last, |(secs, _)| secs);
    head.iter()
        .chain(std::iter::once(&seconds))
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// A run of prose rather than a column name — the backstop for log lines that
/// carry neither a level nor a timestamp. Both bounds must be exceeded: real
/// headers get long (`total_amount_due_usd`) and do contain spaces (`First
/// Name`), but a column name of six or more words is essentially unheard of.
///
/// The thresholds lean towards rejecting, because the two failure directions are
/// not symmetric: a wrongly rejected CSV degrades to `line` and says so via the
/// "No input format detected" hint, whereas a wrongly accepted log line is
/// silent — nonsense field names, exit 0.
fn is_prose_like(field: &str) -> bool {
    field.chars().count() > 30 && field.split_whitespace().count() >= 6
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::strategy::{BoxedStrategy, Strategy};

    #[test]
    fn test_detect_json() {
        assert_eq!(
            detect_format(r#"{"key": "value", "num": 42}"#).unwrap(),
            ConfigInputFormat::Json
        );
        assert_eq!(
            detect_format(r#"{"timestamp": "2023-04-15T10:00:00Z"}"#).unwrap(),
            ConfigInputFormat::Json
        );
    }

    #[test]
    fn test_detect_cef() {
        assert_eq!(
            detect_format("CEF:0|Vendor|Product|Version|EventID|Name|Severity|Extension").unwrap(),
            ConfigInputFormat::Cef
        );
    }

    #[test]
    fn test_detect_syslog() {
        assert_eq!(
            detect_format("<34>1 2023-04-15T10:00:00.000Z hostname app - - - message").unwrap(),
            ConfigInputFormat::Syslog
        );
        assert_eq!(
            detect_format("<13>Apr 15 10:00:00 hostname program: message").unwrap(),
            ConfigInputFormat::Syslog
        );
        // Test syslog format without priority field (common in processed logs)
        assert_eq!(
            detect_format("Jan 15 10:30:45 server1 sshd[1234]: Accepted publickey for user")
                .unwrap(),
            ConfigInputFormat::Syslog
        );
        assert_eq!(
            detect_format("Dec 25 23:59:59 hostname kernel: USB disconnect").unwrap(),
            ConfigInputFormat::Syslog
        );
    }

    #[test]
    fn test_detect_combined() {
        assert_eq!(
            detect_format(
                r#"192.168.1.1 - - [15/Apr/2023:10:00:00 +0000] "GET /path HTTP/1.1" 200 1234"#
            )
            .unwrap(),
            ConfigInputFormat::Combined
        );
    }

    #[test]
    fn test_detect_logfmt() {
        assert_eq!(
            detect_format("time=2023-04-15T10:00:00Z level=info msg=test").unwrap(),
            ConfigInputFormat::Logfmt
        );
        assert_eq!(
            detect_format("key1=value1 key2=value2 key3=value3").unwrap(),
            ConfigInputFormat::Logfmt
        );
        // Two pairs is the detection minimum.
        assert_eq!(
            detect_format("level=info port=8080").unwrap(),
            ConfigInputFormat::Logfmt
        );
    }

    #[test]
    fn test_assignment_shaped_lines_are_not_logfmt() {
        // The parser accepts all of these (every whitespace token contains
        // '='), and must keep doing so under an explicit -f logfmt. Detection
        // must not: each is a single assignment-shaped token, not a logfmt
        // record, and one such line used to recruit logfmt into an
        // auto-detected cascade for the whole file.
        for line in [
            // URL with a query string.
            "https://api.example.com/v1/search?q=timeout&limit=10",
            // Base64 padding.
            "dGhpcyBpcyBhIGJhc2U2NCBibG9iIHdpdGggcGFkZGluZw==",
            // Environment/assignment dump.
            "PATH=/usr/local/bin:/usr/bin:/bin",
            // CLI flag.
            "--config=/etc/app/app.yaml",
            // Single genuine-looking pair: below the two-pair minimum.
            "cache=hit",
            "x==",
        ] {
            assert_eq!(
                detect_format(line).unwrap(),
                ConfigInputFormat::Line,
                "assignment-shaped line must not detect as logfmt: {line}"
            );
        }
    }

    #[test]
    fn test_unterminated_quote_is_not_logfmt() {
        // The parser tolerates an unclosed quote (the value runs to end of
        // line) so a truncated line under -f logfmt still salvages, but for
        // detection an unclosed quote swallowing prose is evidence against
        // logfmt — even when enough pairs precede it.
        for line in [
            r#"config="/etc/app.yaml not found, falling back to defaults"#,
            r#"a=1 msg="unterminated quote and then whatever prose follows"#,
        ] {
            assert_eq!(
                detect_format(line).unwrap(),
                ConfigInputFormat::Line,
                "unterminated quote must not detect as logfmt: {line}"
            );
        }
        // The same shapes with the quote closed are logfmt again.
        assert_eq!(
            detect_format(r#"a=1 msg="now properly terminated""#).unwrap(),
            ConfigInputFormat::Logfmt
        );
    }

    #[test]
    fn test_syslog_requires_a_real_timestamp() {
        // `\w{3}` used to accept any three-letter word as a month and any
        // \d{2}:\d{2}:\d{2} as a clock, fabricating ts fields from lines that
        // were never syslog.
        for line in [
            "Job 15 12:00:00 worker task: completed",
            "Run 27 08:15:00 stage deploy: promoting build",
            "foo 1 10:00:00 h a: b",
            "Feb 30 99:99:99 host app: impossible clock",
            "Jan 0 10:00:00 host app: day zero",
            "Jan 45 10:00:00 host app: day forty-five",
        ] {
            assert_eq!(
                detect_format(line).unwrap(),
                ConfigInputFormat::Line,
                "line with a fake timestamp must not detect as syslog: {line}"
            );
        }
        // Genuine RFC3164 timestamps keep working, including single-digit days
        // and lowercase month spellings.
        for line in [
            "Jan 15 10:30:45 server1 sshd[1234]: Accepted publickey",
            "Dec 25 23:59:59 hostname kernel: USB disconnect",
            "Jan 5 00:00:00 host app: single-digit day",
            "jan 15 10:30:45 host app: lowercase month",
        ] {
            assert_eq!(
                detect_format(line).unwrap(),
                ConfigInputFormat::Syslog,
                "genuine syslog line must still detect: {line}"
            );
        }
    }

    #[test]
    fn test_detect_csv() {
        assert!(matches!(
            detect_format("name,age,city").unwrap(),
            ConfigInputFormat::Csv(_)
        ));
        assert!(matches!(
            detect_format("1,2,3").unwrap(),
            ConfigInputFormat::Csvnh
        ));
        assert!(matches!(
            detect_format("john\t25\tnyc").unwrap(),
            ConfigInputFormat::Tsv(_)
        )); // "john" has letters, so it's treated as header
        assert!(matches!(
            detect_format("name\tage\tcity").unwrap(),
            ConfigInputFormat::Tsv(_)
        ));
        assert!(matches!(
            detect_format("1\t2\t3").unwrap(),
            ConfigInputFormat::Tsvnh
        ));
        // All numeric, no headers
    }

    #[test]
    fn test_app_log_lines_are_not_claimed_as_csv() {
        // A log line containing a comma must not have its first line eaten as a
        // CSV header row. Falling through to `line` (or a named app-log format)
        // is the honest outcome; the with-header CSV guess silently turns every
        // field name into a fragment of a log message.
        for line in [
            // Spark: comma from JVM list rendering. Timestamp + level + prose.
            "17/06/09 20:10:40 INFO executor.Backend: Registered signal handlers for [TERM,HUP,INT]",
            // Comma from a comma-separated value in the message text.
            "17/06/09 20:10:41 INFO spark.SecurityManager: Changing view acls to: yarn,curi",
            // Comma inside a path.
            "17/06/09 20:10:42 INFO storage.DiskBlockManager: Created local dir at /a/b,c",
            // Level but no timestamp.
            "INFO Registered signal handlers for [TERM,HUP,INT]",
            // Timestamp but no level.
            "2024-01-02 15:04:05 shutting down workers a,b,c",
            // Neither: caught by the prose backstop.
            "the quick brown fox jumped over the lazy dog, the cat, and the mouse",
            // A full ISO datetime is a timestamp even alone in its field — no
            // column is named `2024-01-02T10:00:00Z`.
            "2024-01-02T10:00:00Z,ERROR,connection refused",
            "2024-01-02T10:00:00.123+02:00,a,b",
        ] {
            assert!(
                !matches!(
                    detect_format(line).unwrap(),
                    ConfigInputFormat::Csv(_) | ConfigInputFormat::Tsv(_)
                ),
                "log line was claimed as with-header CSV/TSV: {line}"
            );
        }
    }

    #[test]
    fn test_header_guard_keeps_real_csv_headers() {
        // Short identifier headers, headers with spaces, and headers whose names
        // are timestamp/level *vocabulary* (as opposed to timestamp/level-shaped
        // values) must all still detect as csv/tsv.
        for line in [
            "name,age,city",
            "First Name,Last Name,Email",
            "timestamp,level,message",
            "date,time,created_at,errors,warnings",
            "total_amount_due_usd,customer_lifetime_value,churn_probability",
            "\"name\",\"age\",\"city\"",
            // All-caps warehouse/SQL export. `ERROR_COUNT` must not read as a
            // bare `ERROR`, and a column named exactly `ERROR` is still a name.
            "TIMESTAMP,ERROR_COUNT,WARN_COUNT",
            "DATE,ERROR,WARNING",
            // Wide time-series: the column names are dates. Legitimate, and a
            // date-anywhere test would wrongly reject it.
            "region,2024-01-01,2024-01-02",
            // Same shape with clock-valued columns.
            "host,09:00:00,10:00:00",
        ] {
            assert!(
                matches!(detect_format(line).unwrap(), ConfigInputFormat::Csv(_)),
                "real CSV header no longer detected: {line}"
            );
        }
        assert!(matches!(
            detect_format("timestamp\tlevel\tmessage").unwrap(),
            ConfigInputFormat::Tsv(_)
        ));
    }

    #[test]
    fn test_header_guard_does_not_touch_headerless_variants() {
        // csvnh/tsvnh name fields positionally, so there is no header to corrupt
        // — a genuine CSV export of log records must keep working.
        assert!(matches!(
            detect_format("2024-01-02 10:00:00,INFO,started").unwrap(),
            ConfigInputFormat::Csvnh
        ));
        assert!(matches!(
            detect_format("2024-01-02 10:00:00\tINFO\tstarted").unwrap(),
            ConfigInputFormat::Tsvnh
        ));
    }

    #[test]
    fn test_detect_lnav_named_formats() {
        // Application-log layouts that would previously fall through to `line`
        // are now detected as named, regex-backed formats (and the notice/stats
        // show the name rather than a bare "regex").
        for (line, expected) in [
            (
                "2024-01-02T15:04:05.123Z INFO Starting service on port 8080",
                "iso8601-level",
            ),
            (
                "2024-01-02 15:04:05,123 INFO [main] com.example.Service - up",
                "log4j",
            ),
            (
                "2024-01-02 15:04:05,123 - myapp.module - INFO - Service started",
                "python-logging",
            ),
            (
                "2024/01/02 15:04:05 [error] 29#29: *1 open() failed",
                "nginx-error",
            ),
            (
                "I0102 15:04:05.123456 1234 server.go:42] Starting controller",
                "glog",
            ),
        ] {
            match detect_format(line).unwrap() {
                ConfigInputFormat::Named(fmt) => {
                    assert_eq!(fmt.name, expected, "wrong named format for: {line}")
                }
                other => panic!("expected named format {expected} for {line}, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_detect_cri() {
        // JSON message: the commas inside it would trip the CSV detector, but the
        // dedicated CRI step runs first.
        match detect_format(r#"2024-07-17T12:12:05.123456789Z stdout F {"level":"info","a":"b"}"#)
            .unwrap()
        {
            ConfigInputFormat::Named(fmt) => assert_eq!(fmt.name, "cri"),
            other => panic!("expected cri for JSON-message CRI line, got {other:?}"),
        }
        // Plaintext message, stderr stream, partial (P) tag.
        match detect_format("2024-07-17T12:12:06.223456789Z stderr P panic: nil pointer").unwrap() {
            ConfigInputFormat::Named(fmt) => assert_eq!(fmt.name, "cri"),
            other => panic!("expected cri for plaintext CRI line, got {other:?}"),
        }
        // Numeric timezone offset instead of Z.
        match detect_format("2024-07-17T12:12:06.223+02:00 stdout F hello").unwrap() {
            ConfigInputFormat::Named(fmt) => assert_eq!(fmt.name, "cri"),
            other => panic!("expected cri for offset-timezone CRI line, got {other:?}"),
        }
    }

    #[test]
    fn test_cri_does_not_shadow_plain_iso_logs() {
        // A normal ISO-8601 + level application log has no stdout/stderr + F/P
        // marker, so it must still detect as iso8601-level, not cri.
        match detect_format("2024-01-02T15:04:05.123Z INFO Starting service").unwrap() {
            ConfigInputFormat::Named(fmt) => assert_eq!(fmt.name, "iso8601-level"),
            other => panic!("expected iso8601-level, got {other:?}"),
        }
    }

    #[test]
    fn test_lnav_detection_does_not_shadow_existing_formats() {
        // Formats detected before the lnav step must keep their classification.
        assert_eq!(
            detect_format(r#"{"a":1}"#).unwrap(),
            ConfigInputFormat::Json
        );
        assert_eq!(
            detect_format("Jan 15 10:30:45 server1 sshd[1234]: Accepted").unwrap(),
            ConfigInputFormat::Syslog
        );
        assert_eq!(
            detect_format("level=info msg=hi count=1").unwrap(),
            ConfigInputFormat::Logfmt
        );
    }

    /// Assert a `SampleDetection` is `cascade(expected, line)` with no
    /// dropped formats.
    fn assert_capped_cascade(detection: &SampleDetection, expected: &ConfigInputFormat) {
        match &detection.format {
            ConfigInputFormat::Cascade(members) => {
                assert_eq!(members.len(), 2, "auto cascades hold exactly two members");
                assert_eq!(
                    members[0].to_display_string(),
                    expected.to_display_string(),
                    "wrong dominant format"
                );
                assert_eq!(
                    members[1],
                    ConfigInputFormat::Line,
                    "catch-all must be line"
                );
            }
            other => panic!("expected cascade, got {other:?}"),
        }
        assert!(
            detection.unparsed_formats.is_empty() && detection.suggested_cascade.is_none(),
            "no formats should have been dropped: {:?}",
            detection.unparsed_formats
        );
    }

    #[test]
    fn test_sample_homogeneous_matches_single_line_detection() {
        // A sample where every line detects the same way must return exactly
        // what single-line detection returns — files that detect cleanly today
        // are unaffected by sampling.
        assert_eq!(
            detect_format_from_sample([r#"{"a":1}"#, r#"{"b":2}"#, r#"{"c":3}"#])
                .unwrap()
                .format,
            ConfigInputFormat::Json
        );
        assert_eq!(
            detect_format_from_sample(["level=info msg=a", "level=warn msg=b"])
                .unwrap()
                .format,
            ConfigInputFormat::Logfmt
        );
        assert_eq!(
            detect_format_from_sample(["plain text", "more plain text"])
                .unwrap()
                .format,
            ConfigInputFormat::Line
        );
        assert_eq!(
            detect_format_from_sample([]).unwrap().format,
            ConfigInputFormat::Line
        );
    }

    #[test]
    fn test_sample_mixed_builds_cascade_with_line_last() {
        // The headline case: JSON mixed with plain-text lines becomes a
        // cascade, catch-all last, regardless of which shape comes first.
        for sample in [
            vec![r#"{"a":1}"#, "Server starting up...", r#"{"b":2}"#],
            vec!["Server starting up...", r#"{"a":1}"#],
        ] {
            let detection = detect_format_from_sample(sample.iter().copied()).unwrap();
            assert_capped_cascade(&detection, &ConfigInputFormat::Json);
        }
    }

    #[test]
    fn test_sample_caps_cascade_at_one_structured_format() {
        // Two structured formats in the sample: only the dominant one joins
        // the cascade (most lines; ties broken by detection specificity — here
        // json over logfmt). The other is reported for the hint, with a
        // complete, correctly ordered `-f` suggestion.
        let sample = [
            "level=info msg=starting",
            r#"{"level":"error"}"#,
            r#"{"level":"warn"}"#,
            "something unstructured happened",
        ];
        let detection = detect_format_from_sample(sample).unwrap();
        match &detection.format {
            ConfigInputFormat::Cascade(members) => {
                assert_eq!(
                    members,
                    &vec![ConfigInputFormat::Json, ConfigInputFormat::Line]
                );
            }
            other => panic!("expected cascade, got {other:?}"),
        }
        assert_eq!(detection.unparsed_formats, vec!["logfmt".to_string()]);
        assert_eq!(
            detection.suggested_cascade.as_deref(),
            Some("json,logfmt,line")
        );
    }

    #[test]
    fn test_sample_line_member_is_always_present() {
        // Even when no sampled line fell through to `line`, the auto-built
        // cascade still ends in the catch-all: the sample is a guess from a
        // bounded window, so a line past it must never become a parse error
        // on the strength of that guess.
        let sample = [r#"{"a":1}"#, r#"{"b":2}"#, "level=info msg=hi"];
        let detection = detect_format_from_sample(sample).unwrap();
        match &detection.format {
            ConfigInputFormat::Cascade(members) => {
                assert_eq!(
                    members,
                    &vec![ConfigInputFormat::Json, ConfigInputFormat::Line]
                );
            }
            other => panic!("expected cascade, got {other:?}"),
        }
        assert_eq!(detection.unparsed_formats, vec!["logfmt".to_string()]);
    }

    #[test]
    fn test_sample_quorum_ignores_a_single_outlier_line() {
        // One structured-looking line in a big sample must not recruit a
        // parser for the whole file — that single accidental member used to
        // change how every other line was parsed. The dropped format is still
        // reported so the hint can offer the explicit cascade.
        let mut sample: Vec<String> = (0..40)
            .map(|i| format!("Processing batch {i} of 40 records"))
            .collect();
        sample.push("cache=warm ttl=300".to_string());
        let detection = detect_format_from_sample(sample.iter().map(|s| s.as_str())).unwrap();
        assert_eq!(detection.format, ConfigInputFormat::Line);
        assert_eq!(detection.unparsed_formats, vec!["logfmt".to_string()]);
        assert_eq!(detection.suggested_cascade.as_deref(), Some("logfmt,line"));

        // A second supporting line meets the quorum and anchors the cascade.
        sample.push("cache=cold ttl=60".to_string());
        let detection = detect_format_from_sample(sample.iter().map(|s| s.as_str())).unwrap();
        assert_capped_cascade(&detection, &ConfigInputFormat::Logfmt);
    }

    #[test]
    fn test_sample_tiny_samples_skip_the_quorum() {
        // In a file of only two or three lines, a single structured line is a
        // meaningful fraction of the file, not an outlier.
        let detection = detect_format_from_sample([r#"{"a":1}"#, "plain text line"]).unwrap();
        assert_capped_cascade(&detection, &ConfigInputFormat::Json);
    }

    #[test]
    fn test_sample_first_line_csv_short_circuits() {
        // A schema format owns the whole file: sampling must not try to build
        // a cascade around it, even though its data rows re-detect as
        // assorted csv variants.
        assert!(matches!(
            detect_format_from_sample(["name,age,city", "john,25,nyc", "1,2,3"])
                .unwrap()
                .format,
            ConfigInputFormat::Csv(_)
        ));
        assert!(matches!(
            detect_format_from_sample(["2024-01-02 10:00:00,INFO,started", "a,b,c"])
                .unwrap()
                .format,
            ConfigInputFormat::Csvnh
        ));
    }

    #[test]
    fn test_sample_later_csv_looking_lines_count_as_line() {
        // A comma-heavy line mid-file can't be parsed as CSV mid-stream (no
        // header, no fixed schema), so it joins the cascade as `line` rather
        // than poisoning detection.
        let sample = [r#"{"a":1}"#, "1,2,3", r#"{"b":2}"#];
        let detection = detect_format_from_sample(sample).unwrap();
        assert_capped_cascade(&detection, &ConfigInputFormat::Json);
    }

    #[test]
    fn test_sample_named_formats_join_cascades() {
        // log4j lines plus stray unstructured lines: the named format keeps
        // its identity inside the cascade.
        let sample = [
            "2024-01-02 15:04:05,123 INFO [main] com.example.Service - up",
            "some bare continuation text",
        ];
        let detection = detect_format_from_sample(sample).unwrap();
        match &detection.format {
            ConfigInputFormat::Cascade(members) => {
                assert_eq!(members.len(), 2);
                match &members[0] {
                    ConfigInputFormat::Named(fmt) => assert_eq!(fmt.name, "log4j"),
                    other => panic!("expected named log4j first, got {other:?}"),
                }
                assert_eq!(members[1], ConfigInputFormat::Line);
            }
            other => panic!("expected cascade, got {other:?}"),
        }
    }

    #[test]
    fn test_detect_line_fallback() {
        assert_eq!(
            detect_format("just some random text").unwrap(),
            ConfigInputFormat::Line
        );
        assert_eq!(detect_format("").unwrap(), ConfigInputFormat::Line);
        assert_eq!(
            detect_format("a single word").unwrap(),
            ConfigInputFormat::Line
        );
    }

    fn lower_ascii(len: std::ops::RangeInclusive<usize>) -> BoxedStrategy<String> {
        prop::collection::vec(proptest::char::range('a', 'z'), len)
            .prop_map(|chars| chars.into_iter().collect())
            .boxed()
    }

    fn identifier() -> BoxedStrategy<String> {
        lower_ascii(1..=8)
    }

    fn json_value() -> BoxedStrategy<serde_json::Value> {
        let string_val = lower_ascii(0..=8)
            .prop_map(serde_json::Value::String)
            .boxed();

        let number_val = any::<i64>()
            .prop_map(|v| serde_json::Value::Number(serde_json::Number::from(v)))
            .boxed();

        let bool_val = any::<bool>().prop_map(serde_json::Value::Bool).boxed();

        prop_oneof![string_val, number_val, bool_val].boxed()
    }

    fn json_line() -> BoxedStrategy<String> {
        prop::collection::vec((identifier(), json_value()), 1..=4)
            .prop_map(|entries| {
                let mut map = serde_json::Map::new();
                for (k, v) in entries {
                    map.insert(k, v);
                }
                serde_json::Value::Object(map).to_string()
            })
            .boxed()
    }

    fn cef_line() -> BoxedStrategy<String> {
        (
            identifier(),
            identifier(),
            identifier(),
            identifier(),
            identifier(),
            0u8..=10,
            identifier(),
            identifier(),
        )
            .prop_map(|(vendor, product, version, signature, name, severity, ext_key, ext_value)| {
                format!(
                    "CEF:0|{vendor}|{product}|{version}|{signature}|{name}|{severity}|{ext_key}={ext_value}"
                )
            })
            .boxed()
    }

    fn csv_with_headers() -> BoxedStrategy<String> {
        prop::collection::vec(identifier(), 3..=5)
            .prop_map(|fields| fields.join(","))
            .boxed()
    }

    fn csv_without_headers() -> BoxedStrategy<String> {
        prop::collection::vec(0u16..=999, 3..=5)
            .prop_map(|nums| {
                nums.into_iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .boxed()
    }

    fn tsv_with_headers() -> BoxedStrategy<String> {
        prop::collection::vec(identifier(), 3..=5)
            .prop_map(|fields| fields.join("\t"))
            .boxed()
    }

    fn tsv_without_headers() -> BoxedStrategy<String> {
        prop::collection::vec(0u16..=999, 3..=5)
            .prop_map(|nums| {
                nums.into_iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .boxed()
    }

    fn plain_line() -> BoxedStrategy<String> {
        lower_ascii(5..=30)
    }

    proptest! {
        #[test]
        fn prop_detects_json(line in json_line()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Json);
        }

        #[test]
        fn prop_detects_cef(line in cef_line()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Cef);
        }

        #[test]
        fn prop_detects_csv_headers(line in csv_with_headers()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Csv(None));
        }

        #[test]
        fn prop_detects_csv_no_headers(line in csv_without_headers()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Csvnh);
        }

        #[test]
        fn prop_detects_tsv_headers(line in tsv_with_headers()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Tsv(None));
        }

        #[test]
        fn prop_detects_tsv_no_headers(line in tsv_without_headers()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Tsvnh);
        }

        #[test]
        fn prop_detects_line_fallback(line in plain_line()) {
            prop_assert_eq!(detect_format(&line).unwrap(), ConfigInputFormat::Line);
        }
    }
}
