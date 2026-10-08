//! Built-in named log formats, adapted from the lnav project.
//!
//! ## Attribution
//!
//! The log-format catalogue that inspired this module ships with
//! [lnav](https://lnav.org) by Timothy Stack and is distributed under the
//! BSD-3-Clause license. The regular expressions below were re-expressed in
//! Kelora's [`RegexParser`] syntax (`(?P<name>...)` named captures with optional
//! `:type` annotations) and adapted to Kelora's field-naming conventions
//! (`ts`, `level`, `msg`). See `THIRD_PARTY_LICENSES.md` for the full
//! license text and attribution.
//!
//! ## Why this exists
//!
//! Kelora's structured detectors (json, cef, syslog, combined, logfmt, csv)
//! cover wire formats and access logs, but many common *application* log layouts
//! — an ISO-8601 timestamp followed by a level and a message, log4j/Java,
//! Python `logging`, nginx error logs, glog/klog — previously fell through to
//! the plain `line` parser. This module recognises a small, curated set of those
//! layouts and hands them off to the existing [`RegexParser`], so auto-detection
//! extracts structured fields instead of a single opaque `line`.
//!
//! ## Design notes (kept deliberately small)
//!
//! - Each definition is one or more anchored regexes tried against the first
//!   non-empty line. Matching is all-or-nothing — there is **no** lnav-style
//!   weighted scoring engine here; the first definition that matches wins, and
//!   within a definition the first pattern that matches wins.
//! - Multiple patterns per format exist only for sources that emit structurally
//!   distinct layouts (e.g. AWS S3 `std`/`std-v2`, HAProxy http/tcp), which
//!   cannot be folded into one regex because Rust's engine forbids reusing a
//!   capture-group name across alternation branches.
//! - Detection runs after the wire-format detectors (json, cef, syslog,
//!   combined, cri, logfmt) and before csv and the `line` fallback (see
//!   [`crate::parsers::auto_detect`]). Ahead of csv because csv claims any line
//!   with two commas, and app logs often have them. (Consequence:
//!   syslog-transported formats like HAProxy are claimed by the syslog detector
//!   under `-f auto`; reach them with `-f <name>`.)
//! - A match is returned as `InputFormat::Named`, carrying the format's name and
//!   patterns, so it can be displayed, selected via `-f <name>`, and reused
//!   across the parser-build/timestamp/strict pipeline unchanged.
//! - Every definition carries sample lines that are verified at test time, the
//!   same self-validation idea lnav uses for its format files.

use crate::parsers::RegexParser;
use crate::pipeline::EventParser;

/// A built-in named log format: an anchored [`RegexParser`] pattern plus
/// representative sample lines used for self-validation.
///
/// The `name` is a stable, user-facing identifier: it is shown in the
/// auto-detect notice, accepted by `-f <name>` (including inside cascade
/// lists), and listed in `--help-formats`. `samples` are consumed only by the
/// self-validation tests, but are kept as part of the definition so each entry
/// is self-describing and test-verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LnavFormat {
    /// Stable, user-facing identifier (also usable with `-f`).
    pub name: &'static str,
    /// One or more patterns in `RegexParser` syntax, tried in order (first match
    /// wins). Outer `^...$` anchors are added by `RegexParser`, so they are
    /// omitted here. The timestamp is captured into a group named `ts` to match
    /// the field name kelora's other parsers emit. Most formats need a single
    /// pattern; multiple entries are for sources that emit structurally distinct
    /// line layouts (e.g. AWS S3 `std`/`std-v2`) which cannot share one regex
    /// because Rust forbids reusing a capture-group name across alternations.
    pub patterns: &'static [&'static str],
    /// Optional strftime format for the captured `ts` field, applied only when
    /// the user has not passed an explicit `--ts-format`. Applies to every
    /// pattern, so all of a format's layouts must share one timestamp shape. Set
    /// this only for layouts kelora's adaptive timestamp parser does *not* already
    /// recognise (e.g. glog's year-less `MMDD HH:MM:SS.ffffff`); leave it `None`
    /// when the default adaptive parser resolves the timestamp on its own.
    pub ts_format: Option<&'static str>,
    /// Lines this format must parse. Used by the self-validation test and as
    /// living documentation of what each format looks like.
    #[allow(dead_code)] // consumed only by the self-validation tests
    pub samples: &'static [&'static str],
}

/// Curated set of application log formats, ordered from most specific to most
/// general. Order matters: the generic ISO-8601 catch-all is intentionally last
/// so the structured layouts (Java/log4j, etc.) claim their lines first.
///
/// ## Naming convention (these names are user-facing API — see the guard test)
///
/// A format name is the `-f <name>` value, the `_format` field value in
/// cascades, and what `--stats` displays, so renaming one later is a breaking
/// change. New names must follow:
///
/// - lowercase ASCII, digits, and `-` only; start with a letter. No `:` (taken
///   by `cols:`/`regex:`/`csv:` field specs) and no `,` (cascade separator).
/// - must not collide with a built-in format keyword (`json`, `line`, `syslog`,
///   `cef`, `csv`, `cols`, `regex`, `auto`, …).
/// - bare token only when the name is itself a specific, canonical identifier
///   (`glog`, `log4j`). Otherwise use `source-subtype` (`nginx-error`,
///   `python-logging`) and leave the bare family word (`nginx`, `python`, `java`)
///   free for future siblings. Reserve purely structural names (`iso8601-level`)
///   for true generics with no single canonical source.
///
/// No built-in aliases: each format has exactly one name. Users wanting a
/// shorthand can alias at the shell or `.kelora.ini` level.
pub static LNAV_FORMATS: &[LnavFormat] = &[
    // glog / klog (Go, Kubernetes): `I0102 15:04:05.123456 1234 server.go:42] msg`
    // glog omits the year and timezone; its `MMDD HH:MM:SS.ffffff` layout is not
    // in the adaptive parser's list, so we pin the format here (the year-less
    // path in timestamp.rs then assumes the current year, like syslog does).
    LnavFormat {
        name: "glog",
        patterns: &[
            r"(?P<level>[IWEF])(?P<ts>\d{4} \d{2}:\d{2}:\d{2}\.\d{1,6})\s+(?P<pid:int>\d+)\s+(?P<source>[^:\s]+:\d+)\]\s+(?P<msg>.*)",
        ],
        ts_format: Some("%m%d %H:%M:%S%.f"),
        samples: &[
            "I0102 15:04:05.123456 1234 server.go:42] Starting controller",
            "E0612 09:10:11.000001 7 reflector.go:138] Failed to watch",
        ],
    },
    // Kubernetes CRI / containerd on-disk container log (also `kubectl logs
    // --timestamps`): `2024-07-17T12:12:05.123456789Z stdout F <message>`.
    // Layout is `<RFC3339Nano> <stream> <tag> <message>` where stream is
    // stdout/stderr and tag is F (full line) or P (partial — a line the runtime
    // split because it exceeded ~16 KiB). The message is frequently itself JSON
    // or logfmt; like the other named formats this keeps it verbatim in `msg`
    // for an optional second-stage parse. The RFC3339 timestamp is one the
    // adaptive parser already resolves, so no ts_format is pinned.
    //
    // NOTE: unlike the entries above, this layout is *not* from lnav — it is a
    // Kelora-original definition for the Kubernetes container-runtime log format.
    // It reuses the same LnavFormat machinery only because the mechanism fits.
    LnavFormat {
        name: "cri",
        patterns: &[
            r"(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:?\d{2}))\s+(?P<stream>stdout|stderr)\s+(?P<tag>[FP])\s+(?P<msg>.*)",
        ],
        ts_format: None,
        samples: &[
            r#"2024-07-17T12:12:05.123456789Z stdout F {"level":"info","msg":"started"}"#,
            "2024-07-17T12:12:06.223456789Z stderr P panic: runtime error: nil pointer",
        ],
    },
    // nginx error log: `2024/01/02 15:04:05 [error] 29#29: *1 open() failed`
    LnavFormat {
        name: "nginx-error",
        patterns: &[
            r"(?P<ts>\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2})\s+\[(?P<level>\w+)\]\s+(?P<pid:int>\d+)#(?P<tid:int>\d+):\s*(?P<msg>.*)",
        ],
        ts_format: None,
        samples: &[
            "2024/01/02 15:04:05 [error] 29#29: *1 open() failed (2: No such file or directory)",
            "2024/06/12 08:00:00 [warn] 12#0: using uninitialized variable",
        ],
    },
    // Apache httpd error log:
    // `[Wed Oct 11 14:32:52.123456 2024] [core:error] [pid 35:tid 4] [client 1.2.3.4:60223] msg`
    // One regex covers both Apache 2.4 (module:level, pid/tid, client:port) and the
    // older 2.2 layout (bare level, no pid/client) via optional groups. The 2.4
    // timestamp carries subseconds the adaptive parser doesn't know, so pin it
    // (the optional `%.f` also matches the 2.2 timestamp, which has none).
    //
    // The timestamp is spelled out (Apache's `Www Mmm DD HH:MM:SS[.ffffff] YYYY`;
    // Apache 1.3 pads a one-digit day with a space: `Sun Aug  1 04:02:11 2004`)
    // rather than "anything in brackets": the looser form also claimed other
    // `[ts] [x] ...` layouts — spdlog's `[2024-01-02 ...] [logger] [info] msg`,
    // pacman's `[2024-...] [ALPM] ...` — and reported the logger name as `level`.
    LnavFormat {
        name: "apache-error",
        patterns: &[
            r"\[(?P<ts>[A-Z][a-z]{2} [A-Z][a-z]{2} [ \d]\d \d{2}:\d{2}:\d{2}(?:\.\d{1,6})? \d{4})\] \[(?:(?P<module>[^:\]]+):)?(?P<level>\w+)\](?: \[pid (?P<pid:int>\d+)(?::tid (?P<tid:int>\d+))?\])?(?: \[client (?P<client>[^\]]+)\])? (?P<msg>.*)",
        ],
        ts_format: Some("%a %b %d %H:%M:%S%.f %Y"),
        samples: &[
            // Weekday must match the date: chrono validates %a, and Oct 11 2024 is a Friday.
            "[Fri Oct 11 14:32:52.123456 2024] [core:error] [pid 35708:tid 4328636416] [client 72.15.99.187:60223] AH00126: Invalid URI in request",
            "[Fri Oct 11 14:32:52 2024] [error] [client 72.15.99.187] File does not exist: /var/www/favicon.ico",
            // Apache 1.3: space-padded day
            "[Sun Aug  1 04:02:11 2004] [notice] suEXEC mechanism enabled (wrapper: /usr/sbin/suexec)",
        ],
    },
    // log4j / Java: `2024-01-02 15:04:05,123 INFO [main] com.example.Foo - msg`
    LnavFormat {
        name: "log4j",
        patterns: &[
            r"(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3})\s+(?P<level>TRACE|DEBUG|INFO|WARN|ERROR|FATAL)\s+\[(?P<thread>[^\]]+)\]\s+(?P<logger>\S+)\s+-\s+(?P<msg>.*)",
        ],
        ts_format: None,
        samples: &[
            "2024-01-02 15:04:05,123 INFO [main] com.example.Service - Service started",
            "2024-06-12 08:00:00,001 ERROR [pool-1-thread-2] com.acme.Db - Connection refused",
        ],
    },
    // Python logging default: `2024-01-02 15:04:05,123 - myapp - INFO - msg`
    LnavFormat {
        name: "python-logging",
        patterns: &[
            r"(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d{3})\s+-\s+(?P<logger>\S+)\s+-\s+(?P<level>DEBUG|INFO|WARNING|ERROR|CRITICAL)\s+-\s+(?P<msg>.*)",
        ],
        ts_format: None,
        samples: &[
            "2024-01-02 15:04:05,123 - myapp.module - INFO - Service started",
            "2024-06-12 08:00:00,500 - root - ERROR - Unhandled exception",
        ],
    },
    // PostgreSQL server log with the default `log_line_prefix = '%m [%p] '`:
    // `2024-01-02 15:04:05.123 UTC [1234] LOG:  message`.
    // Layout is `<ts> <log_tz> [<pid>] <LEVEL>:  <message>` where log_tz is the
    // configured log_timezone abbreviation (kept verbatim in `log_tz`, outside the
    // `ts` capture so the timestamp stays clean and the adaptive parser resolves
    // it). Postgres uses two spaces after the colon, so `:\s+` is tolerant.
    //
    // Timezone caveat: the naive `ts` is resolved via `--input-tz` (default UTC),
    // *not* the logged abbreviation. We deliberately do not apply `log_tz`: a zone
    // abbreviation can't be reliably converted to an offset (CST/BST/IST are
    // ambiguous, and an abbreviation alone can't encode DST), and chrono's `%Z`
    // can't parse one into an offset — so pinning a ts_format here would just make
    // the timestamp fail to parse. This matches every other naive-timestamp format
    // (log4j, python-logging, syslog). UTC-logged Postgres (the common/recommended
    // config) is therefore correct by default; for a server logging a non-UTC zone,
    // pass `--input-tz <IANA>` (e.g. `--input-tz Europe/Berlin`). The field is named
    // `log_tz` (not `tz`) precisely so it reads as "the abbreviation the server
    // logged", not "the zone applied to this timestamp"; it is kept as a
    // ground-truth label for inspection.
    //
    // NOTE: log_line_prefix is operator-configurable. This matches PostgreSQL's
    // default `%m [%p] ` and Debian/Ubuntu's packaged `%m [%p] %q%u@%d `, whose
    // session lines add `user@db` (captured as `user` and `db`; server-process
    // lines omit it because of `%q`). Other prefixes (app name, client host, …)
    // won't match — reach those with `-f regex:`.
    LnavFormat {
        name: "postgres",
        patterns: &[
            r"(?P<ts>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d{1,6})?) (?P<log_tz>[A-Z]{2,5}) \[(?P<pid:int>\d+)\] (?:(?P<user>[^@\s]+)@(?P<db>\S+) )?(?P<level>LOG|ERROR|FATAL|PANIC|WARNING|NOTICE|INFO|DEBUG[1-5]?|STATEMENT|DETAIL|HINT|CONTEXT):\s+(?P<msg>.*)",
        ],
        ts_format: None,
        samples: &[
            "2024-01-02 15:04:05.123 UTC [1234] LOG:  database system is ready to accept connections",
            "2024-06-12 08:00:00.001 UTC [5678] ERROR:  relation \"users\" does not exist",
            // Debian/Ubuntu package prefix `%m [%p] %q%u@%d `
            "2026-10-08 10:24:32.373 UTC [5871] app@shop ERROR:  relation \"order_items\" does not exist at character 15",
            "2026-10-08 10:24:30.101 UTC [5866] [unknown]@[unknown] LOG:  connection received: host=127.0.0.1 port=50412",
        ],
    },
    // Redis (3.0+): `pid:role date level msg`, e.g.
    // `12345:M 06 Feb 2024 12:00:00.123 * Ready to accept connections`.
    // role is X/C/S/M (sentinel/child/slave/master); level is a single glyph
    // (`.` debug, `-` verbose, `*` notice, `#` warning) that kelora keeps verbatim.
    // The `DD Mon YYYY HH:MM:SS.mmm` timestamp isn't in the adaptive list, so pin it.
    LnavFormat {
        name: "redis",
        patterns: &[
            r"(?P<pid:int>\d+):(?P<role>[XCSM])\s+(?P<ts>\d{1,2} [A-Za-z]{3} \d{4} \d{2}:\d{2}:\d{2}\.\d{3})\s+(?P<level>[.\-*#])\s+(?P<msg>.*)",
        ],
        ts_format: Some("%d %b %Y %H:%M:%S%.f"),
        samples: &[
            "12345:M 06 Feb 2024 12:00:00.123 * Ready to accept connections",
            "12345:M 06 Feb 2024 12:00:01.001 # WARNING overcommit_memory is set to 0",
        ],
    },
    // AWS S3 server access log. Two layouts: the classic `std` fields and the
    // longer `std-v2` (adds version-id/host-id/TLS/auth/host columns). They share
    // field names so they must be separate patterns. The bracketed
    // `DD/Mon/YYYY:HH:MM:SS +zzzz` timestamp is the Apache form the adaptive
    // parser already knows. No level field (access logs have none).
    LnavFormat {
        name: "s3",
        patterns: &[
            // std-v2 (longer) is tried first so the shorter std doesn't match a prefix.
            r#"(?P<owner>\S+)\s+(?P<bucket>\S+)\s+\[(?P<ts>[^\]]+)\]\s+(?P<client>[\w*.:\-]+)\s+(?P<requester>\S+)\s+(?P<req_id>\S+)\s+(?P<op>\S+)\s+(?P<key>\S+)\s+"(?P<method>\S+)\s+(?P<uri>[^ ?]+)(?:\?(?P<query>[^ ]*))?\s+(?P<httpver>\S+)"\s+(?P<status>\d+|-)\s+(?P<error_code>\S+)\s+(?P<bytes_sent>\d+|-)\s+(?P<obj_size>\d+|-)\s+(?P<total_time>\d+|-)\s+(?P<turnaround_time>\d+|-)\s+"(?P<referer>.*?)"\s+"(?P<user_agent>.*?)"\s+(?P<version_id>\S+)\s+(?P<host_id>\S+)\s+(?P<sig_version>\S+)\s+(?P<cipher_suite>\S+)\s+(?P<auth_type>\S+)\s+(?P<host_header>\S+)\s+(?P<tls_version>\S+)"#,
            r#"(?P<owner>\S+)\s+(?P<bucket>\S+)\s+\[(?P<ts>[^\]]+)\]\s+(?P<client>[\w*.:\-]+)\s+(?P<requester>\S+)\s+(?P<req_id>\S+)\s+(?P<op>\S+)\s+(?P<key>\S+)\s+"(?P<method>\S+)\s+(?P<uri>[^ ?]+)(?:\?(?P<query>[^ ]*))?\s+(?P<httpver>\S+)"\s+(?P<status>\d+|-)\s+(?P<error_code>\S+)\s+(?P<bytes_sent>\d+|-)\s+(?P<obj_size>\d+|-)\s+(?P<total_time>\d+|-)\s+(?P<turnaround_time>\d+|-)\s+"(?P<referer>.*?)"\s+"(?P<user_agent>.*?)""#,
        ],
        ts_format: None,
        samples: &[
            r#"79a59df900b949e55d96a1e698fbacedfd6e09d98eacf8f8d5218e7cd47ef2be mybucket [06/Feb/2024:00:00:38 +0000] 192.0.2.3 79a59df900b949e55d96a1e698fbacedfd6e09d98eacf8f8d5218e7cd47ef2be 3E57427F33A59F07 REST.GET.OBJECT photos/cat.jpg "GET /photos/cat.jpg?x-id=GetObject HTTP/1.1" 200 - 2662 2662 14 12 "-" "aws-cli/2.0" - host-id-xyz SigV4 ECDHE-RSA AuthHeader mybucket.s3.amazonaws.com TLSv1.2"#,
            r#"79a59df900b949e55d96a1e698fbacedfd6e09d98eacf8f8d5218e7cd47ef2be mybucket [06/Feb/2024:00:01:00 +0000] 192.0.2.3 - 891CE47D2EXAMPLE REST.GET.BUCKET - "GET /?list-type=2 HTTP/1.1" 200 - 1024 - 7 6 "-" "aws-sdk-go/1.0""#,
        ],
    },
    // HAProxy traffic logs (HTTP and TCP), as emitted through syslog:
    // `Mmm DD HH:MM:SS host haproxy[pid]: client:port [accept] frontend backend/server ...`.
    // NOTE: these carry a syslog timestamp, so auto-detection classifies them as
    // `syslog` first — use `-f haproxy` explicitly to get the structured fields.
    LnavFormat {
        name: "haproxy",
        patterns: &[
            // HTTP log format
            r#"(?P<ts>\w{3} +\d{1,2} \d{2}:\d{2}:\d{2}) (?P<host>\S+) (?P<proc>\w+)\[(?P<pid:int>\d+)\]: (?P<client_ip>[^:]+):(?P<client_port:int>\d+) \[(?P<accept_date>[^\]]+)\] (?P<frontend>\S+?)~? (?P<backend>[^ /]+)/(?P<server>\S+) (?P<tq>-?\d+)/(?P<tw>-?\d+)/(?P<tc>-?\d+)/(?P<tr>-?\d+)/(?P<tt>\d+) (?P<status>\d{3}|-1) (?P<bytes_read:int>\d+) \S+ \S+ (?P<termination_state>....) (?P<actconn:int>\d+)/(?P<feconn:int>\d+)/(?P<beconn:int>\d+)/(?P<srv_conn:int>\d+)/(?P<retries:int>\d+) (?P<srv_queue:int>\d+)/(?P<backend_queue:int>\d+)(?: \{(?P<req_headers>.*?)\} \{(?P<resp_headers>.*?)\})? "(?P<msg>[^"]*)""#,
            // TCP log format
            r#"(?P<ts>\w{3} +\d{1,2} \d{2}:\d{2}:\d{2}) (?P<host>\S+) (?P<proc>\w+)\[(?P<pid:int>\d+)\]: (?P<client_ip>[^:]+):(?P<client_port:int>\d+) \[(?P<accept_date>[^\]]+)\] (?P<frontend>\S+) (?P<backend>[^ /]+)/(?P<server>\S+) (?P<tw>-?\d+)/(?P<tc>-?\d+)/(?P<tt>\d+) (?P<bytes_read:int>\d+) (?P<termination_state>..) (?P<actconn:int>\d+)/(?P<feconn:int>\d+)/(?P<beconn:int>\d+)/(?P<srv_conn:int>\d+)/(?P<retries:int>\d+) (?P<srv_queue:int>\d+)/(?P<backend_queue:int>\d+)"#,
        ],
        ts_format: None,
        samples: &[
            r#"Feb 06 12:14:14 localhost haproxy[14389]: 10.0.1.2:33317 [06/Feb/2024:12:14:14.655] http-in static/srv1 10/0/30/69/109 200 2750 - - ---- 1/1/1/1/0 0/0 "GET /index.html HTTP/1.1""#,
            r#"Feb 06 12:14:15 localhost haproxy[14389]: 10.0.1.2:33320 [06/Feb/2024:12:14:15.123] tcp-in mysql/db1 0/0/5007 1230 -- 1/1/1/1/0 0/0"#,
        ],
    },
    // Spring Boot's default console/file layout (logback, `defaults.xml`):
    // `%d %5p ${PID} --- [app] [%15.15t] %-40.40logger{39} : %m`, e.g.
    // `2026-10-08T10:22:17.721Z  WARN 1 --- [demo] [nio-8080-exec-7] com.example.DemoController : msg`.
    // Spring Boot 3.x/4.x write an ISO timestamp with offset and `[app] `; 2.x a
    // naive `yyyy-MM-dd HH:mm:ss.SSS` and no app — both resolve adaptively. Thread
    // and logger are padded/left-truncated to fixed widths; the padding is
    // dropped. An application group or tracing correlation id adds more bracket
    // groups; those lines don't match and fall through to `iso8601-level`.
    // Must precede `iso8601-level`, which would otherwise claim these lines
    // with pid, thread and logger left in `msg`. Written from real 2.7, 3.5 and
    // 4.1 captures.
    LnavFormat {
        name: "spring-boot",
        patterns: &[
            r"(?P<ts>\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}\.\d{3}(?:Z|[+-]\d{2}:\d{2})?)\s+(?P<level>ERROR|WARN|INFO|DEBUG|TRACE) (?P<pid:int>\d+) --- (?:\[(?P<app>[^\]]+)\] )?\[\s*(?P<thread>[^\]]*)\] (?P<logger>\S+)\s+: (?P<msg>.*)",
        ],
        ts_format: None,
        samples: &[
            "2026-10-08T10:22:17.721Z  WARN 1 --- [demo] [nio-8080-exec-7] com.example.demo.DemoController          : Order 4711 not found, returning empty result",
            "2026-10-08 10:22:31.616  INFO 1 --- [           main] com.example.demo.DemoApplication         : Starting DemoApplication v0.0.1-SNAPSHOT using Java 21.0.12.1",
        ],
    },
    // Tomcat's console/catalina log (JULI `OneLineFormatter`, fixed in code):
    // `08-Oct-2026 10:27:32.174 SEVERE [main] org.apache.Foo.method msg`.
    // Levels are java.util.logging's (SEVERE, WARNING, INFO, CONFIG, FINE…).
    // `source` is the logging class and method. The `dd-MMM-yyyy` date isn't in
    // the adaptive list, so it is pinned. Written from a real Tomcat 11 capture.
    LnavFormat {
        name: "tomcat",
        patterns: &[
            r"(?P<ts>\d{2}-[A-Z][a-z]{2}-\d{4} \d{2}:\d{2}:\d{2}\.\d{3}) (?P<level>SEVERE|WARNING|INFO|CONFIG|FINE|FINER|FINEST) \[(?P<thread>[^\]]*)\] (?P<source>\S+) (?P<msg>.*)",
        ],
        ts_format: Some("%d-%b-%Y %H:%M:%S%.f"),
        samples: &[
            "08-Oct-2026 10:27:21.755 INFO [main] org.apache.catalina.startup.HostConfig.deployDirectory Deploying web application directory [/usr/local/tomcat/webapps/ROOT]",
            "08-Oct-2026 10:27:32.174 SEVERE [Catalina-utility-2] org.apache.tomcat.util.digester.Digester.fatalError Parse fatal error at line [12] column [3]",
        ],
    },
    // MySQL 8 and MariaDB error log:
    // `2026-10-08T10:20:05.980494Z 0 [System] [MY-015017] [Server] msg` (MySQL;
    // the `[MY-n] [Subsystem]` pair is absent in 5.7) and
    // `2026-10-08 10:23:24 0 [Note] InnoDB: msg` (MariaDB). `thread` is the
    // connection id (0 for the server itself). The docker entrypoint's own
    // `… [Note] [Entrypoint]: …` lines have no thread id and don't match.
    // Adapted from lnav's `mysql_error_log`, checked against real MySQL 8.0/8.4
    // and MariaDB 11.4 captures.
    LnavFormat {
        name: "mysql-error",
        patterns: &[
            r"(?P<ts>\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|[+-]\d{2}:\d{2})?) (?P<thread:int>\d+) \[(?P<level>System|ERROR|Error|Warning|Note)\](?: \[(?P<err_code>MY-\d+)\] \[(?P<subsystem>[^\]]+)\])? (?P<msg>.*)",
        ],
        ts_format: None,
        samples: &[
            "2026-10-08T10:20:05.980494Z 0 [System] [MY-015017] [Server] MySQL Server Initialization - start.",
            "2026-10-08 10:23:50 12 [Warning] Aborted connection 12 to db: 'unconnected' user: 'unauthenticated' host: '127.0.0.1' (This connection closed normally without authentication)",
        ],
    },
    // Monolog's default `LineFormatter` (PHP; Laravel, Symfony):
    // `[%datetime%] %channel%.%level_name%: %message% %context% %extra%`.
    // Plain Monolog writes `2026-10-08T10:32:02.948896+00:00` and always prints
    // context and extra (`[]` when empty); Laravel writes `2026-10-08 10:30:50`
    // and drops empty ones. `context` and `extra` are kept as the JSON text
    // Monolog wrote (parse them with `parse_json()`); `extra` is matched only
    // when flat, which processors' output is. Laravel exceptions span many
    // lines inside `context`; group them with `-M`.
    // Adapted from lnav's `laravel_log`, checked against real Laravel 13 and
    // Monolog 3 captures.
    LnavFormat {
        name: "monolog",
        patterns: &[
            // Context and extra both present (plain Monolog).
            r"\[(?P<ts>\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|[+-]\d{2}:\d{2})?)\] (?P<channel>[\w.-]+?)\.(?P<level>DEBUG|INFO|NOTICE|WARNING|ERROR|CRITICAL|ALERT|EMERGENCY): (?P<msg>.*?) (?P<context>\{.*\}|\[\]) (?P<extra>\{[^{}]*\}|\[\])\s*",
            // At most a context (Laravel drops empty context and extra).
            r"\[(?P<ts>\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|[+-]\d{2}:\d{2})?)\] (?P<channel>[\w.-]+?)\.(?P<level>DEBUG|INFO|NOTICE|WARNING|ERROR|CRITICAL|ALERT|EMERGENCY): (?P<msg>.*?)(?: (?P<context>\{.*\}|\[\]))?\s*",
        ],
        ts_format: None,
        samples: &[
            r#"[2026-10-08T10:32:02.948896+00:00] app.INFO: Worker starting {"php":"8.5.11","queues":["emails","invoices"],"concurrency":4} {"host":"fd9ff9f8d00f","uid":"e191406","process_id":1}"#,
            r#"[2026-10-08 10:30:51] local.WARNING: Unusually large cart {"items":25,"threshold":10} "#,
        ],
    },
    // Generic ISO-8601 prefixed application log (catch-all, kept last):
    // `2024-01-02T15:04:05.123Z INFO message` or `2024-01-02 15:04:05 ERROR message`,
    // with the timestamp optionally wrapped in brackets: `[2024-01-02 15:04:05] WARN message`.
    // The `\[?`/`\]?` sit outside the `ts` capture so the emitted field stays clean.
    LnavFormat {
        name: "iso8601-level",
        patterns: &[
            r"\[?(?P<ts>\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d{1,9})?(?:Z|[+-]\d{2}:?\d{2})?)\]?\s+(?P<level>TRACE|DEBUG|INFO|NOTICE|WARN|WARNING|ERROR|ERR|FATAL|CRIT|CRITICAL)\s+(?P<msg>.*)",
        ],
        ts_format: None,
        samples: &[
            "2024-01-02T15:04:05.123Z INFO Starting service on port 8080",
            "2024-06-12 08:00:00 ERROR database connection lost",
            "[2025-01-15 10:00:00] INFO Application started on :8080",
        ],
    },
];

/// Try to recognise `line` as one of the built-in named formats.
///
/// Returns the matching definition (in declaration order), or `None` if no
/// built-in format applies. Patterns are compiled on demand; this runs once per
/// input during auto-detection, so a fresh compile is cheaper than caching.
pub fn detect(line: &str) -> Option<&'static LnavFormat> {
    (0..LNAV_FORMATS.len())
        .find(|&idx| {
            format_parsers(idx)
                .iter()
                .any(|parser| parser.parse(line).is_ok())
        })
        .map(|idx| &LNAV_FORMATS[idx])
}

/// Compiled detection parsers for one entry of [`LNAV_FORMATS`], built at most
/// once per process and per format.
///
/// Detection is called once per *sampled line* — up to `SAMPLE_MAX_LINES` head
/// lines plus mid-file probes — and every line reaching the lnav step is one no
/// earlier detector claimed, so it is tried against all 13 patterns, including
/// the very large S3 and HAProxy ones. Compiling those per call made head
/// sampling cost hundreds of milliseconds on plain-text files. Compilation is
/// inherent; repeating it is not.
///
/// Caching is per format rather than one eager batch so the callers that need a
/// single format — [`matches_named`] for the dedicated `cri` step, which runs
/// for every sampled line — don't pay to compile the other twelve. A file that
/// detects as json or logfmt never reaches [`detect`] at all.
///
/// A pattern that fails to compile is dropped rather than fatal: it can only
/// make detection miss that format, and `all_formats_parse_their_samples`
/// already fails the test suite if any built-in pattern is malformed.
fn format_parsers(idx: usize) -> &'static [RegexParser] {
    static PARSERS: std::sync::OnceLock<Vec<std::sync::OnceLock<Vec<RegexParser>>>> =
        std::sync::OnceLock::new();
    let cells = PARSERS.get_or_init(|| {
        LNAV_FORMATS
            .iter()
            .map(|_| std::sync::OnceLock::new())
            .collect()
    });
    cells[idx].get_or_init(|| {
        LNAV_FORMATS[idx]
            .patterns
            .iter()
            .filter_map(|pattern| RegexParser::new(pattern).ok())
            .collect()
    })
}

/// Test `line` against a single built-in format, reusing the same cached
/// parsers as [`detect`]. Used by the dedicated `cri` detection step, which runs
/// ahead of the generic lnav step (see [`crate::parsers::auto_detect`]).
pub fn matches_named(name: &str, line: &str) -> Option<&'static LnavFormat> {
    let idx = LNAV_FORMATS
        .iter()
        .position(|fmt| fmt.name.eq_ignore_ascii_case(name))?;
    format_parsers(idx)
        .iter()
        .any(|parser| parser.parse(line).is_ok())
        .then_some(&LNAV_FORMATS[idx])
}

/// Look up a built-in named format by its identifier (e.g. for `-f log4j`).
/// Names are lowercase; lookup is case-insensitive for friendliness.
pub fn by_name(name: &str) -> Option<&'static LnavFormat> {
    LNAV_FORMATS
        .iter()
        .find(|fmt| fmt.name.eq_ignore_ascii_case(name))
}

/// Comma-separated list of built-in format names, for help and error text.
pub fn names_csv() -> String {
    LNAV_FORMATS
        .iter()
        .map(|fmt| fmt.name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Self-validation: every definition must compile and parse all of its own
    /// sample lines. Mirrors lnav's requirement that a format's samples match.
    #[test]
    fn all_formats_parse_their_samples() {
        for fmt in LNAV_FORMATS {
            let parser = crate::parsers::MultiRegexParser::new(fmt.patterns, false)
                .unwrap_or_else(|e| panic!("format '{}' failed to compile: {e}", fmt.name));
            for sample in fmt.samples {
                assert!(
                    parser.parse(sample).is_ok(),
                    "format '{}' did not parse its sample: {sample:?}",
                    fmt.name
                );
            }
        }
    }

    /// Each pattern of a multi-pattern format must be reachable: every sample is
    /// expected to be claimed by exactly one pattern, and across the samples all
    /// patterns should fire (guards against a dead/shadowed pattern).
    #[test]
    fn multi_pattern_formats_exercise_every_pattern() {
        for fmt in LNAV_FORMATS {
            if fmt.patterns.len() < 2 {
                continue;
            }
            let compiled: Vec<RegexParser> = fmt
                .patterns
                .iter()
                .map(|p| RegexParser::new(p).expect("pattern compiles"))
                .collect();
            let mut hit = vec![false; compiled.len()];
            for sample in fmt.samples {
                // First matching pattern (same order the parser uses) wins.
                let idx = compiled
                    .iter()
                    .position(|p| p.parse(sample).is_ok())
                    .unwrap_or_else(|| {
                        panic!("'{}' sample matched no pattern: {sample:?}", fmt.name)
                    });
                hit[idx] = true;
            }
            assert!(
                hit.iter().all(|&h| h),
                "format '{}' has a pattern no sample exercises ({hit:?}); add a covering sample",
                fmt.name
            );
        }
    }

    /// Each sample must be claimed by its own format first (detection order is
    /// correct and the definitions do not shadow one another).
    #[test]
    fn samples_detect_to_their_own_format() {
        for fmt in LNAV_FORMATS {
            for sample in fmt.samples {
                let detected = detect(sample)
                    .unwrap_or_else(|| panic!("sample not detected at all: {sample:?}"));
                assert_eq!(
                    detected.name, fmt.name,
                    "sample {sample:?} detected as '{}' but belongs to '{}'",
                    detected.name, fmt.name
                );
            }
        }
    }

    #[test]
    fn extracts_expected_fields() {
        let fmt = detect("2024-01-02 15:04:05,123 INFO [main] com.example.Service - up").unwrap();
        let parser = crate::parsers::MultiRegexParser::new(fmt.patterns, false).unwrap();
        let event = parser
            .parse("2024-01-02 15:04:05,123 INFO [main] com.example.Service - up")
            .unwrap();
        assert_eq!(
            event
                .fields
                .get("level")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "INFO"
        );
        assert_eq!(
            event
                .fields
                .get("thread")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "main"
        );
        assert_eq!(
            event
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "up"
        );
    }

    #[test]
    fn bracketed_timestamp_app_log_extracts_clean_fields() {
        // The common `[timestamp] LEVEL message` app-log shape (e.g. examples/app.log)
        // must detect as iso8601-level, and the brackets must stay out of `ts`.
        let line = "[2025-01-15 10:00:00] INFO Application started on :8080";
        let fmt = detect(line).expect("bracketed app log should detect");
        assert_eq!(fmt.name, "iso8601-level");

        let parser = crate::parsers::MultiRegexParser::new(fmt.patterns, false).unwrap();
        let event = parser.parse(line).unwrap();

        let field = |name: &str| {
            event
                .fields
                .get(name)
                .unwrap_or_else(|| panic!("missing field {name}"))
                .clone()
                .into_string()
                .unwrap()
        };
        assert_eq!(field("ts"), "2025-01-15 10:00:00");
        assert_eq!(field("level"), "INFO");
        assert_eq!(field("msg"), "Application started on :8080");
    }

    #[test]
    fn does_not_match_plain_text_or_csv() {
        // Plain prose and CSV-of-words must not be misdetected.
        assert!(detect("just some random log text here").is_none());
        assert!(detect("name,age,city").is_none());
        assert!(detect("hello world").is_none());
        // A bare date without a level should not match the generic format.
        assert!(detect("2024-01-02T15:04:05Z just a message without level").is_none());
    }

    #[test]
    fn apache_error_requires_apache_timestamp() {
        // Other `[timestamp] [word] ...` layouts must not be read as Apache error
        // logs: the bracketed word would land in `level`.
        for line in [
            "[2014-10-31 23:46:59.678] [my_loggername] [info] Some message", // spdlog
            "[2024-01-02T15:04:05+0100] [ALPM] installed foo (1.0-1)",       // pacman
        ] {
            assert_ne!(
                detect(line).map(|fmt| fmt.name),
                Some("apache-error"),
                "{line:?} must not detect as apache-error"
            );
        }
    }

    #[test]
    fn postgres_debian_prefix_extracts_user_and_db() {
        let line = "2026-10-08 10:24:34.463 UTC [5885] app@shop FATAL:  password authentication failed for user \"app\"";
        let fmt = detect(line).expect("Debian postgres line should detect");
        assert_eq!(fmt.name, "postgres");
        let parser = crate::parsers::MultiRegexParser::new(fmt.patterns, false).unwrap();
        let event = parser.parse(line).unwrap();
        let field = |name: &str| event.fields.get(name).unwrap().to_string();
        assert_eq!(field("user"), "app");
        assert_eq!(field("db"), "shop");
        assert_eq!(field("level"), "FATAL");
        assert_eq!(
            field("msg"),
            "password authentication failed for user \"app\""
        );

        // Server-process lines carry no user@db; the fields are absent.
        let line = "2026-10-08 10:24:27.285 UTC [5844] LOG:  database system is ready to accept connections";
        let event = parser.parse(line).unwrap();
        assert!(!event.fields.contains_key("user"));
        assert!(!event.fields.contains_key("db"));
    }

    #[test]
    fn monolog_splits_context_and_extra() {
        let fmt = by_name("monolog").unwrap();
        let parser = crate::parsers::MultiRegexParser::new(fmt.patterns, false).unwrap();
        let field = |line: &str, name: &str| {
            parser
                .parse(line)
                .unwrap()
                .fields
                .get(name)
                .map(|v| v.to_string())
        };

        // Plain Monolog: context and extra, nested JSON in the context.
        let line = r#"[2026-10-08T10:32:02.948896+00:00] app.INFO: Worker starting {"queues":["emails"],"cfg":{"n":4}} {"host":"fd9f","process_id":1}"#;
        assert_eq!(field(line, "msg").as_deref(), Some("Worker starting"));
        assert_eq!(
            field(line, "context").as_deref(),
            Some(r#"{"queues":["emails"],"cfg":{"n":4}}"#)
        );
        assert_eq!(
            field(line, "extra").as_deref(),
            Some(r#"{"host":"fd9f","process_id":1}"#)
        );

        // Empty context and extra print as `[]`.
        let line = "[2026-10-08T10:32:02.968000+00:00] app.DEBUG: Loaded configuration [] []";
        assert_eq!(field(line, "msg").as_deref(), Some("Loaded configuration"));

        // Laravel: context only, trailing space; or neither.
        let line = r#"[2026-10-08 10:30:51] local.WARNING: Unusually large cart {"items":25} "#;
        assert_eq!(field(line, "msg").as_deref(), Some("Unusually large cart"));
        assert_eq!(field(line, "context").as_deref(), Some(r#"{"items":25}"#));
        assert_eq!(field(line, "extra"), None);
        let line = "[2026-10-08 10:30:51] local.INFO: Inventory sync requested  ";
        assert_eq!(
            field(line, "msg").as_deref(),
            Some("Inventory sync requested")
        );
        assert_eq!(field(line, "context"), None);
    }

    #[test]
    fn typed_fields_are_converted() {
        // glog pid is annotated :int and should arrive as an integer.
        let fmt = detect("I0102 15:04:05.123456 1234 server.go:42] hi").unwrap();
        let parser = crate::parsers::MultiRegexParser::new(fmt.patterns, false).unwrap();
        let event = parser
            .parse("I0102 15:04:05.123456 1234 server.go:42] hi")
            .unwrap();
        assert_eq!(event.fields.get("pid").unwrap().as_int().unwrap(), 1234);
    }

    #[test]
    fn every_format_captures_a_ts_field() {
        // The timestamp group is named `ts` to match kelora's other parsers, so
        // the standard timestamp field-name detection picks it up.
        for fmt in LNAV_FORMATS {
            let parser = crate::parsers::MultiRegexParser::new(fmt.patterns, false).unwrap();
            for sample in fmt.samples {
                let event = parser.parse(sample).unwrap();
                assert!(
                    event.fields.contains_key("ts"),
                    "format '{}' should capture a 'ts' field from {sample:?}",
                    fmt.name
                );
            }
        }
    }

    #[test]
    fn ts_format_pinned_only_where_adaptive_cant_resolve() {
        // A format pins ts_format only when its timestamp shape isn't in the
        // adaptive parser's list (glog's year-less MMDD, redis' `DD Mon YYYY`,
        // apache's `%a %b %d ... %Y` with subseconds). Everything else is None.
        for fmt in LNAV_FORMATS {
            let expected = match fmt.name {
                "glog" => Some("%m%d %H:%M:%S%.f"),
                "redis" => Some("%d %b %Y %H:%M:%S%.f"),
                "apache-error" => Some("%a %b %d %H:%M:%S%.f %Y"),
                "tomcat" => Some("%d-%b-%Y %H:%M:%S%.f"),
                _ => None,
            };
            assert_eq!(
                fmt.ts_format, expected,
                "unexpected ts_format for '{}'",
                fmt.name
            );
        }
    }

    /// Guards the naming convention documented on `LNAV_FORMATS`. These names are
    /// user-facing API, so a new format must not silently break the rules.
    #[test]
    fn names_follow_convention_and_dont_collide() {
        // Built-in `-f` keywords a named format must never shadow (see
        // config::parse_input_format_spec / cli::parse_format_value).
        const RESERVED: &[&str] = &[
            "auto",
            "auto-per-file",
            "json",
            "line",
            "raw",
            "logfmt",
            "syslog",
            "cef",
            "csv",
            "tsv",
            "csvnh",
            "tsvnh",
            "combined",
            "cols",
            "regex",
            "cascade",
        ];

        let mut seen = std::collections::HashSet::new();
        for fmt in LNAV_FORMATS {
            let name = fmt.name;

            // Unique across the catalogue.
            assert!(seen.insert(name), "duplicate format name '{name}'");

            // Never collide with a reserved keyword.
            assert!(
                !RESERVED.contains(&name),
                "format name '{name}' collides with a built-in format keyword"
            );

            // Charset: start with a letter, then lowercase ASCII / digits / '-'.
            // This also enforces "no ':' and no ','" implicitly.
            assert!(
                name.starts_with(|c: char| c.is_ascii_lowercase()),
                "format name '{name}' must start with a lowercase letter"
            );
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "format name '{name}' must be lowercase ASCII letters, digits, or '-'"
            );
            assert!(
                !name.ends_with('-') && !name.contains("--"),
                "format name '{name}' has a stray or doubled '-'"
            );
        }
    }
}
