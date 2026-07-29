# drain-rs

Drain provides a mechanism for online log categorization.

This version provides:

- serialization/deserialization of drain state via serde json
- support for GROK patterns for more accurate categories and variable filtering

The goal of this particular project is to provide a nice, fast, rust upgrade to the original [drain](https://github.com/logpai/logparser/tree/master/logparser/Drain) implementation.
Original paper here:
- Pinjia He, Jieming Zhu, Zibin Zheng, and Michael R. Lyu. [Drain: An Online Log Parsing Approach with Fixed Depth Tree](http://jmzhu.logpai.com/pub/pjhe_icws2017.pdf), Proceedings of the 24th International Conference on Web Services (ICWS), 2017.


This is a WIP, 0.2.x

## Installing

```rust
[dependencies]
drain-rs = "0.2.0"
```

## Using drain for clustering

To use drain for clustering:

```rust
//Create new drain tree object
let mut drain = DrainTree::new()
// Add log lines and see their group:
let log_group = drain.add_log_line(s.as_str());
```

To use drain with grok:
```rust
let mut g = grok::Grok::with_patterns();
let filter_patterns = vec![
    "blk_(|-)[0-9]+",     //blockid
    "%{IPV4:ip_address}", //IP
    "%{NUMBER:number}",   //Num
];
// Build new drain tree
let mut drain = DrainTree::new()
    .filter_patterns(filter_patterns)
    .max_depth(4)
    .max_children(100)
    .min_similarity(0.5)
    // HDFS log pattern, variable format printout in the content section
    .log_pattern("%{NUMBER:date} %{NUMBER:time} %{NUMBER:proc} %{LOGLEVEL:level} %{DATA:component}: %{GREEDYDATA:content}", "content")
    // Compile all the grok patterns so that they can be used
    .build_patterns(&mut g);
```
