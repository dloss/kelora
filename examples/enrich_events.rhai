// This script enriches log events with additional computed fields

// Add timestamp metadata
if e.has("timestamp") {
    let ts = e.timestamp;
    if ts.contains("T") {
        let parts = ts.split("T");
        e.date = parts[0];
        if parts.len() > 1 {
            e.time = parts[1].split("Z")[0];
        }
    }
}

// Classify by severity
if e.has("level") {
    e.severity = if e.level == "ERROR" || e.level == "CRITICAL" {
        "high"
    } else if e.level == "WARN" {
        "medium"
    } else {
        "low"
    };
}

// Add duration classification if present
// Handle response_time (seconds), duration_ms (milliseconds), or latency_ms (milliseconds)
let duration_in_ms = if e.has("response_time") {
    e.response_time * 1000.0
} else if e.has("duration_ms") {
    e.duration_ms
} else if e.has("latency_ms") {
    e.latency_ms
} else {
    -1.0
};

if duration_in_ms >= 0.0 {
    e.speed = if duration_in_ms < 100.0 {
        "fast"
    } else if duration_in_ms < 1000.0 {
        "normal"
    } else {
        "slow"
    };
}

// Track by service and level
if e.has("service") && e.has("level") {
    track_count("service_level", e.service + ":" + e.level);
}

// Return enriched event
e
