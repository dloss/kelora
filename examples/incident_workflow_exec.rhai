// Classify issue type
e.issue_type = if e.level == "ERROR" {
  "error"
} else {
  "latency"
};

// Track by type and service
track_freq("issue_type", e.issue_type);
track_top("service_issue", e.service + ":" + e.issue_type, 10);
track_stats("response_time", e.get_path("response_time", 0.0));
