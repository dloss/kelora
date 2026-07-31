use crate::event::Event;
use crate::pipeline::EventParser;
use anyhow::{Context, Result};
use regex::{Regex, RegexBuilder};
use rhai::Dynamic;

/// Compile a syslog pattern whose trailing `(.*)` message must be able to span
/// the newlines a multiline join inserts. See the call site for why.
fn multiline_aware_regex(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .dot_matches_new_line(true)
        .build()
}

pub struct SyslogParser {
    rfc5424_regex: Regex,
    rfc3164_regex: Regex,
    auto_timestamp: bool,
}

impl SyslogParser {
    fn build(auto_timestamp: bool) -> Result<Self> {
        // `s` (dot matches newline) is on so the trailing message capture can hold
        // a multi-line event — with `--multiline-join=newline` the joiner puts
        // newlines inside the event, and a line-bounded `.` would reject every
        // stack trace as "Invalid syslog format" (#368). `multi_line` stays off,
        // so `^`/`$` still anchor to the ends of the whole event.
        let rfc5424_regex = multiline_aware_regex(
            r"^<(\d{1,3})>(\d+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)(?:\s+(.*))?(?:\r?\n)?$",
        )
        .context("Failed to compile RFC5424 regex")?;

        // The timestamp validates for real: a named month (any case) instead of
        // `\w{3}`, a 1-31 day, and a 24h clock. `\w{3}` accepted any 3-letter
        // word, so `Job 15 12:00:00 worker task: completed` parsed as syslog
        // with `ts='Job 15 12:00:00'` — a fabricated timestamp that then failed
        // chrono parsing anyway. No real RFC3164 line is rejected by the
        // tightening; only never-were-syslog lines are.
        let rfc3164_regex = multiline_aware_regex(
            r"^(?:<(\d{1,3})>)?((?i:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+(?:[12]\d|3[01]|0?[1-9])\s+(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d)\s+(\S+)\s+([^:\[\s]+)(?:\[(\d+)\])?\s*:\s*(.*)(?:\r?\n)?$"
        ).context("Failed to compile RFC3164 regex")?;

        Ok(Self {
            rfc5424_regex,
            rfc3164_regex,
            auto_timestamp,
        })
    }

    pub fn new() -> Result<Self> {
        Self::build(true)
    }

    pub fn new_without_auto_timestamp() -> Result<Self> {
        Self::build(false)
    }

    /// Parse priority value into facility and severity
    fn parse_priority(priority: u32) -> (u32, u32) {
        let facility = priority >> 3;
        let severity = priority & 7;
        (facility, severity)
    }

    /// Map syslog severity (0-7) to log level string
    fn severity_to_level(severity: u32) -> &'static str {
        match severity {
            0 => "EMERG",
            1 => "ALERT",
            2 => "CRIT",
            3 => "ERROR",
            4 => "WARN",
            5 => "NOTICE",
            6 => "INFO",
            7 => "DEBUG",
            _ => "UNKNOWN",
        }
    }

    /// Try to parse as RFC5424 format first
    fn try_parse_rfc5424(&self, line: &str) -> Option<Event> {
        if let Some(captures) = self.rfc5424_regex.captures(line) {
            let priority_str = captures.get(1)?.as_str();
            let priority: u32 = priority_str.parse().ok()?;

            // Validate priority range (0-191)
            if priority > 191 {
                return None;
            }

            let (facility, severity) = Self::parse_priority(priority);

            // Pre-allocate with expected field count
            let mut event = Event::with_capacity(line.to_string(), 11);

            // Set priority fields
            event.set_field("pri".to_string(), Dynamic::from(priority as i64));
            event.set_field("facility".to_string(), Dynamic::from(facility as i64));
            event.set_field("severity".to_string(), Dynamic::from(severity as i64));
            event.set_field(
                "level".to_string(),
                Dynamic::from(Self::severity_to_level(severity)),
            );

            // Set version
            if let Some(version) = captures.get(2) {
                if let Ok(v) = version.as_str().parse::<i64>() {
                    event.set_field("version".to_string(), Dynamic::from(v));
                }
            }

            // Set timestamp
            if let Some(ts) = captures.get(3) {
                let ts_str = ts.as_str();
                if ts_str != "-" {
                    event.set_field("ts".to_string(), Dynamic::from(ts_str.to_string()));
                }
            }

            // Set hostname
            if let Some(host) = captures.get(4) {
                let host_str = host.as_str();
                if host_str != "-" {
                    event.set_field("host".to_string(), Dynamic::from(host_str.to_string()));
                }
            }

            // Set program name
            if let Some(prog) = captures.get(5) {
                let prog_str = prog.as_str();
                if prog_str != "-" {
                    event.set_field("prog".to_string(), Dynamic::from(prog_str.to_string()));
                }
            }

            // Set process ID
            if let Some(pid) = captures.get(6) {
                let pid_str = pid.as_str();
                if pid_str != "-" {
                    if let Ok(pid_num) = pid_str.parse::<i64>() {
                        event.set_field("pid".to_string(), Dynamic::from(pid_num));
                    } else {
                        event.set_field("pid".to_string(), Dynamic::from(pid_str.to_string()));
                    }
                }
            }

            // Set message ID
            if let Some(msgid) = captures.get(7) {
                let msgid_str = msgid.as_str();
                if msgid_str != "-" {
                    event.set_field("msgid".to_string(), Dynamic::from(msgid_str.to_string()));
                }
            }

            // Set structured data (skip for now, treat as part of message)
            // if let Some(sd) = captures.get(8) {
            //     let sd_str = sd.as_str();
            //     if sd_str != "-" {
            //         event.set_field("sd".to_string(), Dynamic::from(sd_str.to_string()));
            //     }
            // }

            // Set message
            if let Some(msg) = captures.get(9) {
                event.set_field("msg".to_string(), Dynamic::from(msg.as_str().to_string()));
            }

            if self.auto_timestamp {
                event.extract_timestamp();
            }
            Some(event)
        } else {
            None
        }
    }

    /// Try to parse as RFC3164 format
    fn try_parse_rfc3164(&self, line: &str) -> Option<Event> {
        if let Some(captures) = self.rfc3164_regex.captures(line) {
            // Pre-allocate with expected field count
            let mut event = Event::with_capacity(line.to_string(), 8);

            // Set priority fields if present
            if let Some(priority_match) = captures.get(1) {
                let priority: u32 = priority_match.as_str().parse().ok()?;

                // Validate priority range (0-191)
                if priority > 191 {
                    return None;
                }

                let (facility, severity) = Self::parse_priority(priority);

                event.set_field("pri".to_string(), Dynamic::from(priority as i64));
                event.set_field("facility".to_string(), Dynamic::from(facility as i64));
                event.set_field("severity".to_string(), Dynamic::from(severity as i64));
                event.set_field(
                    "level".to_string(),
                    Dynamic::from(Self::severity_to_level(severity)),
                );
            }

            // Set timestamp (group 2 now since priority is group 1)
            if let Some(ts) = captures.get(2) {
                event.set_field("ts".to_string(), Dynamic::from(ts.as_str().to_string()));
            }

            // Set hostname (group 3)
            if let Some(host) = captures.get(3) {
                event.set_field("host".to_string(), Dynamic::from(host.as_str().to_string()));
            }

            // Set program name (group 4)
            if let Some(prog) = captures.get(4) {
                event.set_field("prog".to_string(), Dynamic::from(prog.as_str().to_string()));
            }

            // Set process ID (optional, group 5)
            if let Some(pid) = captures.get(5) {
                if let Ok(pid_num) = pid.as_str().parse::<i64>() {
                    event.set_field("pid".to_string(), Dynamic::from(pid_num));
                } else {
                    event.set_field("pid".to_string(), Dynamic::from(pid.as_str().to_string()));
                }
            }

            // Set message (group 6)
            if let Some(msg) = captures.get(6) {
                event.set_field("msg".to_string(), Dynamic::from(msg.as_str().to_string()));
            }

            if self.auto_timestamp {
                event.extract_timestamp();
            }
            Some(event)
        } else {
            None
        }
    }
}

impl EventParser for SyslogParser {
    fn parse(&self, line: &str) -> Result<Event> {
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        // Try RFC5424 first, then RFC3164
        if let Some(event) = self.try_parse_rfc5424(line) {
            Ok(event)
        } else if let Some(event) = self.try_parse_rfc3164(line) {
            Ok(event)
        } else {
            Err(anyhow::anyhow!("Invalid syslog format"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::EventParser;

    #[test]
    fn test_rfc3164_timestamp_must_be_real() {
        // `\w{3}` used to accept any three-letter word as the month and any
        // \d{2}:\d{2}:\d{2} as the clock, so `Job 15 12:00:00 worker task: …`
        // parsed as syslog with a fabricated `ts` field. The month is now a
        // real month name (any case), the day 1-31, and the clock 24h-valid.
        let parser = SyslogParser::new().unwrap();
        for line in [
            "Job 15 12:00:00 worker task: completed",
            "foo 1 10:00:00 h a: b",
            "Feb 30 99:99:99 host app: impossible clock",
            "Jan 32 10:00:00 host app: day out of range",
            "Jan 15 24:00:00 host app: hour out of range",
        ] {
            assert!(
                EventParser::parse(&parser, line).is_err(),
                "fake timestamp must not parse as RFC3164: {line}"
            );
        }
        for line in [
            "Jan 5 00:00:00 host app: single-digit day",
            "Dec 31 23:59:59 host app: end of year",
            "jan 15 10:30:45 host app: lowercase month",
        ] {
            assert!(
                EventParser::parse(&parser, line).is_ok(),
                "genuine RFC3164 line must still parse: {line}"
            );
        }
    }

    #[test]
    fn test_syslog_parser_rfc5424() {
        let parser = SyslogParser::new().unwrap();
        let line =
            "<165>1 2023-10-11T22:14:15.003Z server01 sshd 1234 ID47 - Failed password for user";
        let result = EventParser::parse(&parser, line).unwrap();

        // Check priority parsing
        assert_eq!(result.fields.get("pri").unwrap().as_int().unwrap(), 165);
        assert_eq!(result.fields.get("facility").unwrap().as_int().unwrap(), 20); // 165 >> 3 = 20
        assert_eq!(result.fields.get("severity").unwrap().as_int().unwrap(), 5); // 165 & 7 = 5

        // Check other fields
        assert_eq!(result.fields.get("version").unwrap().as_int().unwrap(), 1);
        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "2023-10-11T22:14:15.003Z"
        );
        assert_eq!(
            result
                .fields
                .get("host")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "server01"
        );
        assert_eq!(
            result
                .fields
                .get("prog")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "sshd"
        );
        assert_eq!(result.fields.get("pid").unwrap().as_int().unwrap(), 1234);
        assert_eq!(
            result
                .fields
                .get("msgid")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "ID47"
        );
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Failed password for user"
        );
    }

    /// A syslog event assembled by `--multiline-join=newline` carries its
    /// continuation lines inside the message; rejecting it as "Invalid syslog
    /// format" dropped exactly the stack traces users came for (#368).
    #[test]
    fn test_syslog_message_spans_joined_newlines() {
        let parser = SyslogParser::new().unwrap();

        let rfc3164 = EventParser::parse(
            &parser,
            "Oct 11 22:14:15 server01 app[1234]: Payment failed\njava.lang.RuntimeException: timeout\n\tat Client.charge(Client.java:88)",
        )
        .unwrap();
        assert_eq!(
            rfc3164
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Payment failed\njava.lang.RuntimeException: timeout\n\tat Client.charge(Client.java:88)"
        );

        let rfc5424 = EventParser::parse(
            &parser,
            "<165>1 2023-10-11T22:14:15.003Z server01 app 1234 ID47 - Payment failed\n\tat Client.charge(Client.java:88)",
        )
        .unwrap();
        assert_eq!(
            rfc5424
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Payment failed\n\tat Client.charge(Client.java:88)"
        );
    }

    #[test]
    fn test_syslog_parser_rfc3164() {
        let parser = SyslogParser::new().unwrap();
        let line =
            "Oct 11 22:14:15 server01 sshd[1234]: Failed password for user from 192.168.1.100";
        let result = EventParser::parse(&parser, line).unwrap();

        // Check fields
        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Oct 11 22:14:15"
        );
        assert_eq!(
            result
                .fields
                .get("host")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "server01"
        );
        assert_eq!(
            result
                .fields
                .get("prog")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "sshd"
        );
        assert_eq!(result.fields.get("pid").unwrap().as_int().unwrap(), 1234);
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Failed password for user from 192.168.1.100"
        );

        assert!(result.parsed_ts.is_some());
    }

    #[test]
    fn test_syslog_parser_rfc3164_no_pid() {
        let parser = SyslogParser::new().unwrap();
        let line = "Oct 11 22:14:15 server01 kernel: CPU0: Core temperature above threshold";
        let result = EventParser::parse(&parser, line).unwrap();

        // Check fields
        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Oct 11 22:14:15"
        );
        assert_eq!(
            result
                .fields
                .get("host")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "server01"
        );
        assert_eq!(
            result
                .fields
                .get("prog")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "kernel"
        );
        assert!(result.fields.get("pid").is_none()); // No PID in this format
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "CPU0: Core temperature above threshold"
        );
    }

    #[test]
    fn test_syslog_parser_rfc3164_with_priority() {
        let parser = SyslogParser::new().unwrap();
        let line = "<34>Oct 11 22:14:15 webserver nginx: 192.168.1.10 - - [11/Oct/2023:22:14:15 +0000] \"GET /index.html HTTP/1.1\" 200 612";
        let result = EventParser::parse(&parser, line).unwrap();

        // Check priority fields
        assert_eq!(result.fields.get("pri").unwrap().as_int().unwrap(), 34);
        assert_eq!(result.fields.get("facility").unwrap().as_int().unwrap(), 4); // 34 >> 3
        assert_eq!(result.fields.get("severity").unwrap().as_int().unwrap(), 2); // 34 & 7

        // Check other fields
        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Oct 11 22:14:15"
        );
        assert_eq!(
            result
                .fields
                .get("host")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "webserver"
        );
        assert_eq!(
            result
                .fields
                .get("prog")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "nginx"
        );
        assert!(result.fields.get("pid").is_none()); // No PID in this format
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "192.168.1.10 - - [11/Oct/2023:22:14:15 +0000] \"GET /index.html HTTP/1.1\" 200 612"
        );
    }

    #[test]
    fn test_syslog_parser_priority_calculation() {
        let parser = SyslogParser::new().unwrap();

        // Test different priority values
        let test_cases = [
            (0, 0, 0),    // kern.emerg
            (33, 4, 1),   // auth.alert
            (165, 20, 5), // local4.notice
            (191, 23, 7), // local7.debug
        ];

        for (priority, expected_facility, expected_severity) in test_cases {
            let line = format!(
                "<{}>1 2023-10-11T22:14:15.003Z server01 test - - - Test message",
                priority
            );
            let result = EventParser::parse(&parser, &line).unwrap();

            assert_eq!(
                result.fields.get("pri").unwrap().as_int().unwrap(),
                priority as i64
            );
            assert_eq!(
                result.fields.get("facility").unwrap().as_int().unwrap(),
                expected_facility
            );
            assert_eq!(
                result.fields.get("severity").unwrap().as_int().unwrap(),
                expected_severity
            );
        }
    }

    #[test]
    fn test_syslog_parser_invalid_priority() {
        let parser = SyslogParser::new().unwrap();
        let line = "<999>1 2023-10-11T22:14:15.003Z server01 test - - - Test message";
        assert!(EventParser::parse(&parser, line).is_err());
    }

    #[test]
    fn test_syslog_parser_invalid_format() {
        let parser = SyslogParser::new().unwrap();
        let line = "This is not a syslog line";
        assert!(EventParser::parse(&parser, line).is_err());
    }

    #[test]
    fn test_syslog_parser_fallback_to_rfc3164() {
        let parser = SyslogParser::new().unwrap();

        // This should fail RFC5424 parsing and fall back to RFC3164
        let line = "Dec 25 14:09:07 server01 httpd: GET /index.html HTTP/1.1";
        let result = EventParser::parse(&parser, line).unwrap();

        assert_eq!(
            result
                .fields
                .get("ts")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Dec 25 14:09:07"
        );
        assert_eq!(
            result
                .fields
                .get("host")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "server01"
        );
        assert_eq!(
            result
                .fields
                .get("prog")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "httpd"
        );
        assert_eq!(
            result
                .fields
                .get("msg")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "GET /index.html HTTP/1.1"
        );
    }
}
