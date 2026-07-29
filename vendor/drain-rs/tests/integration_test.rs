use drain_rs::DrainTree;

#[test]
fn dump_and_load() {
    let logs = vec![
        "1 [INFO] user 3 called 192.0.0.1",
        "2 [INFO] user 2 called 127.0.0.1",
        "3 [DEBUG] something uninteresting happened",
        "4 [INFO] user 4 called 10.0.0.1",
    ];

    let mut g = grok::Grok::with_patterns();

    let filter_patterns = vec!["%{IPV4:ip_address}", "%{NUMBER:user_id}"];

    let mut drain = DrainTree::new()
        .filter_patterns(filter_patterns)
        .max_depth(4)
        .max_children(100)
        .min_similarity(0.5)
        .log_pattern(
            "%{NUMBER:id} \\[%{LOGLEVEL:level}\\] %{GREEDYDATA:content}",
            "content",
        )
        .build_patterns(&mut g);

    for log in logs {
        drain.add_log_line(log);
    }

    let serialized = serde_json::to_string(&drain).expect("serialization failure");

    let other: DrainTree = serde_json::from_str(serialized.as_str()).unwrap();
    let other = other.build_patterns(&mut g);

    assert_eq!(
        other
            .log_group(&"10 [INFO] user 40 called 192.168.10.2")
            .expect("missing expected log group")
            .as_string(),
        "user <user_id> called <ip_address>"
    );
    assert_eq!(
        other
            .log_group(&"2 [INFO] something uninteresting happened")
            .expect("missing expected log group")
            .as_string(),
        "something uninteresting happened"
    );
}

#[test]
fn log_clustering() {
    let logs = vec![
        "1 [INFO] user 3 called 192.0.0.1",
        "2 [INFO] user 2 called 127.0.0.1",
        "3 [DEBUG] something uninteresting happened",
        "4 [INFO] user 4 called 10.0.0.1",
    ];

    let mut g = grok::Grok::with_patterns();

    let filter_patterns = vec!["%{IPV4:ip_address}", "%{NUMBER:user_id}"];

    let mut drain = DrainTree::new()
        .filter_patterns(filter_patterns)
        .max_depth(4)
        .max_children(100)
        .min_similarity(0.5)
        .log_pattern(
            "%{NUMBER:id} \\[%{LOGLEVEL:level}\\] %{GREEDYDATA:content}",
            "content",
        )
        .build_patterns(&mut g);

    for log in logs {
        drain.add_log_line(log);
    }
    assert_eq!(
        drain
            .log_group(&"10 [INFO] user 40 called 192.168.10.2")
            .expect("missing expected log group")
            .as_string(),
        "user <user_id> called <ip_address>"
    );
    assert_eq!(
        drain
            .log_group(&"2 [INFO] something uninteresting happened")
            .expect("missing expected log group")
            .as_string(),
        "something uninteresting happened"
    );
}
