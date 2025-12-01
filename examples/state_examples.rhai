// state_examples.rhai — Common patterns for using the `state` global map
//
// The `state` global map enables complex stateful processing across events.
// Only works in sequential mode (not with --parallel).
//
// WHEN TO USE `state`:
//   - Deduplication (tracking seen IDs)
//   - Cross-event dependencies
//   - Storing complex objects (nested maps, arrays)
//   - Conditional logic based on previous events
//
// WHEN NOT TO USE `state`:
//   - Simple counting or metrics → use track_count(), track_sum(), etc.
//     (they work in parallel mode too)
//
// Usage:
//   kelora -j logs.jsonl --exec-file examples/state_examples.rhai
//
// See also:
//   - tests/state_integration_test.rs for comprehensive examples
//   - docs/reference/script-variables.md for full API reference
//   - docs/how-to/power-user-techniques.md for use cases

// ============================================================================
// PATTERN 1: Deduplication - Track seen IDs
// ============================================================================
//
// Skip duplicate events based on request_id
//
// if !state.contains(e.request_id) {
//     state[e.request_id] = true;
//     // Process first occurrence
// } else {
//     // Skip duplicate
//     e = ();
// }


// ============================================================================
// PATTERN 2: Sequential numbering across all events
// ============================================================================
//
// Assign a global sequence number to each event
// Use with --begin to initialize:
//   kelora -j logs.jsonl --begin 'state["seq"] = 0' \
//     --exec-file examples/state_examples.rhai
//
// state["seq"] = (state["seq"] ?? 0) + 1;
// e.sequence_number = state["seq"];


// ============================================================================
// PATTERN 3: Track complex per-entity state
// ============================================================================
//
// Store nested maps with multiple attributes per user
//
// if !state.contains(e.user_id) {
//     state[e.user_id] = #{
//         login_count: 0,
//         last_seen: (),
//         errors: [],
//         total_bytes: 0
//     };
// }
//
// let user_state = state[e.user_id];
// user_state.login_count += 1;
// user_state.last_seen = e.timestamp;
// user_state.total_bytes += e.bytes ?? 0;
//
// if e.has("error") {
//     user_state.errors.push(e.error);
// }
//
// state[e.user_id] = user_state;
// e.user_login_count = user_state.login_count;


// ============================================================================
// PATTERN 4: Conditional processing based on previous events
// ============================================================================
//
// Only process events after seeing a specific trigger event
//
// if e.event_type == "session_start" {
//     state["session_active"] = true;
//     state["session_id"] = e.session_id;
// }
//
// if state["session_active"] == true {
//     // Process events within active session
//     e.active_session_id = state["session_id"];
// } else {
//     // Skip events outside session
//     e = ();
// }
//
// if e.event_type == "session_end" {
//     state["session_active"] = false;
// }


// ============================================================================
// PATTERN 5: Convert state to regular map for output
// ============================================================================
//
// Use with --end to print final state:
//   kelora -j logs.jsonl --exec-file examples/state_examples.rhai \
//     --end 'print(state.to_map().to_logfmt())' -q
//
// state[e.level] = (state.get(e.level) ?? 0) + 1;


// ============================================================================
// PATTERN 6: Track first and last occurrence
// ============================================================================
//
// Store both first and last timestamp for each unique ID
//
// if !state.contains(e.request_id) {
//     state[e.request_id] = #{
//         first_seen: e.timestamp,
//         last_seen: e.timestamp,
//         count: 1
//     };
// } else {
//     let req_state = state[e.request_id];
//     req_state.last_seen = e.timestamp;
//     req_state.count += 1;
//     state[e.request_id] = req_state;
// }
//
// e.occurrence_count = state[e.request_id].count;


// ============================================================================
// PATTERN 7: Rate limiting - Only emit first N events per key
// ============================================================================
//
// Limit output to first 100 events per API key
//
// if !state.contains(e.api_key) {
//     state[e.api_key] = 0;
// }
// state[e.api_key] += 1;
// if state[e.api_key] > 100 {
//     e = ();  // Drop after 100 events per API key
// } else {
//     e.event_number = state[e.api_key];
// }


// ============================================================================
// PATTERN 8: Event correlation - Match request/response pairs
// ============================================================================
//
// Calculate latency by matching requests with their responses
//
// if e.event_type == "request" {
//     state[e.request_id] = #{sent_at: e.timestamp, method: e.method, path: e.path};
//     e = ();  // Don't emit until we see response
// } else if e.event_type == "response" && state.contains(e.request_id) {
//     let req = state[e.request_id];
//     e.duration_ms = (e.timestamp - req.sent_at).as_millis();
//     e.method = req.method;
//     e.path = req.path;
//     state.remove(e.request_id);  // Clean up to prevent memory growth
// }


// ============================================================================
// PATTERN 9: State machines for protocol analysis
// ============================================================================
//
// Track TCP connection states through their lifecycle
//
// if !state.contains(e.conn_id) {
//     state[e.conn_id] = "NEW";
// }
// let current_state = state[e.conn_id];
//
// // State transitions
// if current_state == "NEW" && e.event == "SYN" {
//     state[e.conn_id] = "SYN_SENT";
// } else if current_state == "SYN_SENT" && e.event == "SYN_ACK" {
//     state[e.conn_id] = "ESTABLISHED";
// } else if current_state == "ESTABLISHED" && e.event == "FIN" {
//     state[e.conn_id] = "CLOSING";
// } else if current_state == "CLOSING" && e.event == "ACK" {
//     state[e.conn_id] = "CLOSED";
//     state.remove(e.conn_id);  // Connection complete
// } else if e.event != "DATA" {
//     e.protocol_error = true;  // Invalid state transition
// }
// e.connection_state = state[e.conn_id];


// ============================================================================
// PATTERN 10: Session reconstruction - Build complete sessions
// ============================================================================
//
// Accumulate events into sessions, emit only complete sessions
// Use with: kelora -j logs.jsonl --exec-file state_examples.rhai -q
//
// if e.event == "login" {
//     state[e.session_id] = #{
//         user: e.user,
//         events: [],
//         start: e.timestamp,
//         errors: 0
//     };
// }
//
// if state.contains(e.session_id) {
//     let session = state[e.session_id];
//     session.events.push(#{event: e.event, ts: e.timestamp});
//     if e.has("error") {
//         session.errors += 1;
//     }
//     state[e.session_id] = session;
// }
//
// if e.event == "logout" {
//     let session = state[e.session_id];
//     session.end = e.timestamp;
//     session.event_count = session.events.len();
//     session.duration = (session.end - session.start).as_secs();
//     print(session.to_json());  // Emit complete session
//     state.remove(e.session_id);
// }
// e = ();  // Suppress individual events


// ============================================================================
// PATTERN 11: Baseline tracking for anomaly detection
// ============================================================================
//
// Calculate moving average and detect anomalies
//
// if !state.contains("baseline") {
//     state["baseline"] = #{sum: 0.0, count: 0, avg: 0.0};
// }
// let bl = state["baseline"];
// bl.sum += e.response_time;
// bl.count += 1;
// bl.avg = bl.sum / bl.count;
// state["baseline"] = bl;
//
// // Flag anomalies (3x average)
// e.is_anomaly = (e.response_time > bl.avg * 3);
// e.baseline_avg = bl.avg;
// e.deviation = e.response_time / bl.avg;


// ============================================================================
// PATTERN 12: Periodic state cleanup
// ============================================================================
//
// Prevent unbounded memory growth with periodic cleanup
//
// if !state.contains("counter") {
//     state["counter"] = 0;
//     state["last_cleanup"] = now();
// }
// state["counter"] += 1;
//
// // Cleanup every 100k events or every hour
// if state["counter"] % 100000 == 0 {
//     eprint("State size: " + state.len() + " keys");
//
//     if state.len() > 1000000 {
//         state.clear();
//         eprint("State cleared due to size");
//     }
// }
//
// // Time-based cleanup example
// // if (now() - state["last_cleanup"]).as_secs() > 3600 {
// //     // Remove entries older than 1 hour
// //     for key in state.keys() {
// //         if key != "counter" && key != "last_cleanup" {
// //             if should_expire(state[key]) {
// //                 state.remove(key);
// //             }
// //         }
// //     }
// //     state["last_cleanup"] = now();
// // }


// ============================================================================
// AVAILABLE STATE OPERATIONS
// ============================================================================
//
// Indexing:
//   state["key"]                 Get/set values (returns () if not exists)
//   state["key"] = value         Set a value
//
// Methods:
//   state.contains("key")        Check if key exists
//   state.get("key")             Get value (returns () if not exists)
//   state.set("key", value)      Set value
//   state.len()                  Number of keys
//   state.is_empty()             True if no keys
//   state.keys()                 Array of all keys
//   state.values()               Array of all values
//   state.clear()                Remove all entries
//   state.remove("key")          Remove specific key
//
// Operators:
//   state += #{k: v}             Merge map into state
//   state.mixin(#{k: v})         Merge map into state (same as +=)
//   state.fill_with(#{k: v})     Replace entire state with map
//
// Conversion:
//   state.to_map()               Convert to regular Rhai map
//                                (needed for to_logfmt(), to_kv(), etc.)
//
// Debugging:
//   eprint("State size: " + state.len())
//   eprint("Keys: " + state.keys().to_json())
//   print(state.to_map().to_logfmt())  // In --end hook
