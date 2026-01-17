// resolve_fields.rhai — semantic field resolution for cross-format log analysis.
//
// Different log formats use different field names for the same concepts:
//   - Duration: response_time, latency, duration_ms, elapsed, ...
//   - User: user_id, userId, user, uid, username, ...
//   - Client IP: ip, client_ip, remote_addr, source_ip, ...
//
// This module provides functions to resolve these semantic concepts regardless
// of the actual field name used, enabling queries that work across formats.
//
// Functions:
//   resolve_<concept>(event)      → value (or () if not found)
//   resolve_<concept>_name(event) → field name that matched (or "" if not found)
//
// Available concepts: duration, user, client_ip, error, request_id, status
//
// Usage examples:
//   # Filter slow requests regardless of field naming
//   kelora --include examples/resolve_fields.rhai -f json logs.jsonl \
//     --exec 'if resolve_duration(e) > 1000 { e } else { () }'
//
//   # Normalize user field across different log sources
//   kelora --include examples/resolve_fields.rhai -f json logs.jsonl \
//     --exec 'e.normalized_user = resolve_user(e); e'
//
//   # Find which duration field a log format uses
//   kelora --include examples/resolve_fields.rhai -f json logs.jsonl \
//     --exec 'print(resolve_duration_name(e))' --head 1
//
//   # Aggregate by user across mixed formats
//   kelora --include examples/resolve_fields.rhai -f json *.jsonl \
//     --exec 'track_stats(resolve_user(e) ?? "unknown", resolve_duration(e) ?? 0)'
//
// Customization:
//   Copy this file and modify the _get_field_defs() function to match your
//   environment. Add organization-specific field names or remove unused ones.

// ============================================================================
// Field name definitions - customize these for your environment
// ============================================================================

fn _get_field_defs() {
    #{
        duration: [
            "response_time",
            "latency",
            "duration",
            "duration_ms",
            "elapsed",
            "elapsed_ms",
            "time_taken",
            "request_time",
            "exec_time",
            "execution_time",
            "processing_time",
            "rt",
            "took",
        ],
        user: [
            "user_id",
            "userId",
            "user",
            "uid",
            "username",
            "user_name",
            "account_id",
            "accountId",
            "account",
            "actor",
            "principal",
            "subject",
            "sub",
        ],
        client_ip: [
            "ip",
            "client_ip",
            "clientIp",
            "remote_addr",
            "remoteAddr",
            "source_ip",
            "sourceIp",
            "src_ip",
            "srcIp",
            "client_address",
            "peer_ip",
            "x_forwarded_for",
            "xff",
        ],
        error: [
            "error",
            "err",
            "error_message",
            "errorMessage",
            "exception",
            "fault",
            "failure",
            "failed",
        ],
        request_id: [
            "request_id",
            "requestId",
            "req_id",
            "trace_id",
            "traceId",
            "span_id",
            "spanId",
            "correlation_id",
            "correlationId",
            "x_request_id",
            "txn_id",
            "transaction_id",
        ],
        status: [
            "status",
            "status_code",
            "statusCode",
            "http_status",
            "httpStatus",
            "response_code",
            "responseCode",
            "code",
        ],
    }
}

// ============================================================================
// Generic resolver - used by all concept-specific functions
// ============================================================================

fn _resolve_field(event, concept) {
    let defs = _get_field_defs();
    let field_names = defs[concept];
    for name in field_names {
        if event.has(name) {
            return event[name];
        }
    }
    ()
}

fn _resolve_field_name(event, concept) {
    let defs = _get_field_defs();
    let field_names = defs[concept];
    for name in field_names {
        if event.has(name) {
            return name;
        }
    }
    ""
}

// ============================================================================
// Duration - time taken for an operation (response_time, latency, etc.)
// ============================================================================

fn resolve_duration(event) {
    _resolve_field(event, "duration")
}

fn resolve_duration_name(event) {
    _resolve_field_name(event, "duration")
}

// ============================================================================
// User - identity of the actor (user_id, username, etc.)
// ============================================================================

fn resolve_user(event) {
    _resolve_field(event, "user")
}

fn resolve_user_name(event) {
    _resolve_field_name(event, "user")
}

// ============================================================================
// Client IP - network origin of the request
// ============================================================================

fn resolve_client_ip(event) {
    _resolve_field(event, "client_ip")
}

fn resolve_client_ip_name(event) {
    _resolve_field_name(event, "client_ip")
}

// ============================================================================
// Error - error message or indicator
// ============================================================================

fn resolve_error(event) {
    _resolve_field(event, "error")
}

fn resolve_error_name(event) {
    _resolve_field_name(event, "error")
}

// ============================================================================
// Request ID - correlation/trace identifier
// ============================================================================

fn resolve_request_id(event) {
    _resolve_field(event, "request_id")
}

fn resolve_request_id_name(event) {
    _resolve_field_name(event, "request_id")
}

// ============================================================================
// Status - HTTP status code or similar
// ============================================================================

fn resolve_status(event) {
    _resolve_field(event, "status")
}

fn resolve_status_name(event) {
    _resolve_field_name(event, "status")
}

// ============================================================================
// Utility: Check if event has any error indicators
// ============================================================================

// Returns true if the event appears to be an error based on:
// - Presence of error fields
// - Log level indicating error
// - HTTP status codes 4xx/5xx
fn has_error(event) {
    // Check for error-related fields
    if resolve_error(event) != () {
        return true;
    }

    // Check log level
    if event.has("level") {
        let lvl = event.level.to_upper();
        if lvl == "ERROR" || lvl == "ERR" || lvl == "CRITICAL" ||
           lvl == "FATAL" || lvl == "ALERT" || lvl == "EMERG" {
            return true;
        }
    }

    // Check status codes (4xx/5xx)
    let status = resolve_status(event);
    if status != () && status >= 400 {
        return true;
    }

    false
}

// ============================================================================
// Utility: List available concepts
// ============================================================================

fn resolve_field_concepts() {
    let defs = _get_field_defs();
    let concepts = defs.keys();
    concepts.sort();
    concepts
}
