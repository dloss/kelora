//! Template-mining accuracy against the loghub_2k ground truth.
//!
//! Run with `just drain-accuracy` (fetches the corpus, then runs this).
//! `#[ignore]`d because it needs that corpus on disk — like fuzzing, this is a
//! manual measurement, not a CI gate.
//!
//! **Why this exists.** Every change to masking, tree routing or cluster merging
//! moves which lines end up grouped together, and nothing else in the suite can
//! see that: the unit tests pin individual templates, so a change that improves
//! one message and wrecks a hundred others passes them all. This scores the whole
//! corpus, and fails when a dataset regresses against the committed baseline in
//! `dev/drain-accuracy-baseline.json`.
//!
//! **What it measures**, per dataset, and why one number is not enough:
//!
//! - `accuracy` — grouping accuracy, the published metric: the share of events
//!   whose mined cluster contains *exactly* the events of their ground-truth
//!   cluster. Both over-splitting and over-merging lower it.
//! - `templates` vs `clusters` — mined count against true count, which says
//!   which direction the errors run. 129 mined against 27 true is a very
//!   different problem from 6 against 27, and accuracy alone conflates them.
//! - `oversplit` — true clusters spread across several mined templates. Clutter:
//!   the output is longer than it should be, and per-template statistics
//!   (`--drain-diff`) fragment below their reporting floor.
//! - `overmerged` / `overmerged_events` — mined templates that swallow more than
//!   one true cluster, and how many events they cover. **Weighted heaviest when
//!   judging a change**: over-splitting is noise the reader can see through,
//!   over-merging destroys the distinction and cannot be recovered from the
//!   output. A merge pass can trade accuracy up while making this worse, which is
//!   exactly the trade this metric exists to expose.
//!
//! Read both canaries before accepting a change: Linux/OpenSSH/Mac catch
//! over-splitting, HealthApp/Apache catch over-merging. An average that improves
//! while any single dataset falls off a cliff is not an improvement.

use kelora::drain;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Field separator written by `dev/fetch-drain-corpus.sh`.
const US: char = '\x1f';

/// Accuracy may drift by this much before a dataset counts as regressed, so
/// re-running the harness on an unchanged tree never fails on rounding.
const REGRESSION_TOLERANCE: f64 = 0.0005;

struct Dataset {
    name: String,
    /// Ground-truth cluster id per line.
    truth: Vec<String>,
    /// Ground-truth template text per cluster id, for the diagnostic listing.
    truth_templates: HashMap<String, String>,
    /// The message text mined, one per line.
    content: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct Score {
    accuracy: f64,
    templates: usize,
    clusters: usize,
    oversplit: usize,
    overmerged: usize,
    overmerged_events: usize,
}

fn corpus_dir() -> PathBuf {
    match std::env::var_os("KELORA_DRAIN_CORPUS") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from("target/drain-corpus"),
    }
}

fn load(dir: &Path) -> Vec<Dataset> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| {
            panic!(
                "drain corpus not found in {}: {err}\nRun `just drain-accuracy` (or `bash dev/fetch-drain-corpus.sh`) first.",
                dir.display()
            )
        })
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "records"))
        .collect();
    entries.sort();
    assert!(
        !entries.is_empty(),
        "no *.records in {} — run `bash dev/fetch-drain-corpus.sh`",
        dir.display()
    );

    entries
        .iter()
        .map(|path| {
            let name = path
                .file_stem()
                .expect("record file has a stem")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
            let mut truth = Vec::new();
            let mut truth_templates = HashMap::new();
            let mut content = Vec::new();
            for line in text.lines() {
                let mut parts = line.split(US);
                let (Some(id), Some(template), Some(text)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    panic!("malformed record in {}: {line:?}", path.display());
                };
                truth_templates.insert(id.to_string(), template.to_string());
                truth.push(id.to_string());
                content.push(text.to_string());
            }
            Dataset {
                name,
                truth,
                truth_templates,
                content,
            }
        })
        .collect()
}

/// Mine `content` and return each line's template from the *final* model.
///
/// Deliberately not the per-line template `drain_template` returns during
/// ingest: a cluster's template is rewritten as it generalizes, so an
/// ingest-time answer describes a model that no longer exists by end of input.
/// What a user sees from `--drain` — and what `--drain-diff` counts against — is
/// the finished set, so that is what is scored, via the same frozen matcher
/// `--drain-diff` uses for its second pass.
fn assign(content: &[String]) -> Vec<String> {
    drain::reset();
    for (i, text) in content.iter().enumerate() {
        drain::drain_record(text, None, Some(i + 1)).expect("drain_record");
    }
    let frozen = drain::frozen_template_set();
    let assigned = content
        .iter()
        .map(|text| {
            frozen
                .match_text(text)
                .unwrap_or_else(|| panic!("no frozen template matched {text:?}"))
                .to_string()
        })
        .collect();
    drain::reset();
    assigned
}

fn score(truth: &[String], mined: &[String]) -> Score {
    assert_eq!(truth.len(), mined.len());
    let mut truth_groups: HashMap<&str, HashSet<usize>> = HashMap::new();
    let mut mined_groups: HashMap<&str, HashSet<usize>> = HashMap::new();
    for (i, (t, m)) in truth.iter().zip(mined.iter()).enumerate() {
        truth_groups.entry(t).or_default().insert(i);
        mined_groups.entry(m).or_default().insert(i);
    }

    // Grouping accuracy: an event counts only when its mined group is exactly
    // its ground-truth group — same partition, not merely the same label.
    let mut correct = 0usize;
    let mut overmerged = 0usize;
    let mut overmerged_events = 0usize;
    for members in mined_groups.values() {
        let ids: HashSet<&str> = members.iter().map(|i| truth[*i].as_str()).collect();
        if ids.len() > 1 {
            overmerged += 1;
            overmerged_events += members.len();
            continue;
        }
        let only = ids.iter().next().expect("non-empty mined group");
        if truth_groups[*only] == *members {
            correct += members.len();
        }
    }

    let oversplit = truth_groups
        .iter()
        .filter(|(_, members)| {
            members
                .iter()
                .map(|i| mined[*i].as_str())
                .collect::<HashSet<_>>()
                .len()
                > 1
        })
        .count();

    Score {
        accuracy: correct as f64 / truth.len() as f64,
        templates: mined_groups.len(),
        clusters: truth_groups.len(),
        oversplit,
        overmerged,
        overmerged_events,
    }
}

/// The worst offenders, so a regression names the message that moved rather than
/// only the number that moved.
fn diagnostics(dataset: &Dataset, mined: &[String]) -> Vec<String> {
    let mut truth_to_mined: BTreeMap<&str, HashSet<&str>> = BTreeMap::new();
    let mut mined_to_truth: BTreeMap<&str, HashSet<&str>> = BTreeMap::new();
    let mut truth_counts: HashMap<&str, usize> = HashMap::new();
    for (t, m) in dataset.truth.iter().zip(mined.iter()) {
        truth_to_mined.entry(t).or_default().insert(m);
        mined_to_truth.entry(m).or_default().insert(t);
        *truth_counts.entry(t).or_default() += 1;
    }

    let mut out = Vec::new();
    let mut splits: Vec<(usize, &str)> = truth_to_mined
        .iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(k, v)| (v.len(), *k))
        .collect();
    splits.sort_by_key(|(n, id)| (std::cmp::Reverse(*n), *id));
    for (n, id) in splits.iter().take(3) {
        out.push(format!(
            "    oversplit x{n} ({} events): {}",
            truth_counts[*id], dataset.truth_templates[*id]
        ));
    }

    let mut merges: Vec<(usize, &str)> = mined_to_truth
        .iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(k, v)| (v.len(), *k))
        .collect();
    merges.sort_by_key(|(n, tpl)| (std::cmp::Reverse(*n), *tpl));
    for (n, tpl) in merges.iter().take(3) {
        out.push(format!("    overmerged {n} true clusters into: {tpl}"));
    }
    out
}

fn baseline_path() -> PathBuf {
    PathBuf::from("dev/drain-accuracy-baseline.json")
}

fn load_baseline() -> Option<BTreeMap<String, Score>> {
    let text = std::fs::read_to_string(baseline_path()).ok()?;
    let raw: serde_json::Value = serde_json::from_str(&text).ok()?;
    let obj = raw.get("datasets")?.as_object()?;
    let mut out = BTreeMap::new();
    for (name, v) in obj {
        out.insert(
            name.clone(),
            Score {
                accuracy: v.get("accuracy")?.as_f64()?,
                templates: v.get("templates")?.as_u64()? as usize,
                clusters: v.get("clusters")?.as_u64()? as usize,
                oversplit: v.get("oversplit")?.as_u64()? as usize,
                overmerged: v.get("overmerged")?.as_u64()? as usize,
                overmerged_events: v.get("overmerged_events")?.as_u64()? as usize,
            },
        );
    }
    Some(out)
}

fn write_baseline(scores: &BTreeMap<String, Score>) {
    let datasets: serde_json::Map<String, serde_json::Value> = scores
        .iter()
        .map(|(name, s)| {
            (
                name.clone(),
                serde_json::json!({
                    "accuracy": (s.accuracy * 10_000.0).round() / 10_000.0,
                    "templates": s.templates,
                    "clusters": s.clusters,
                    "oversplit": s.oversplit,
                    "overmerged": s.overmerged,
                    "overmerged_events": s.overmerged_events,
                }),
            )
        })
        .collect();
    let mean = scores.values().map(|s| s.accuracy).sum::<f64>() / scores.len() as f64;
    let doc = serde_json::json!({
        "_comment": "Written by `just drain-accuracy-update`. Grouping accuracy against loghub_2k; see tests/drain_accuracy.rs.",
        "mean_accuracy": (mean * 10_000.0).round() / 10_000.0,
        "datasets": datasets,
    });
    std::fs::write(
        baseline_path(),
        serde_json::to_string_pretty(&doc).expect("serialize baseline") + "\n",
    )
    .expect("write baseline");
}

#[test]
#[ignore = "needs the loghub corpus; run via `just drain-accuracy`"]
fn drain_accuracy_against_loghub_ground_truth() {
    let datasets = load(&corpus_dir());
    let baseline = load_baseline();
    let update = std::env::var_os("KELORA_DRAIN_ACCURACY_UPDATE").is_some();

    let mut scores: BTreeMap<String, Score> = BTreeMap::new();
    let mut details: Vec<String> = Vec::new();

    println!(
        "\n{:<13}{:>9}{:>8}{:>10}{:>11}{:>11}{:>13}",
        "dataset", "accuracy", "mined", "true", "oversplit", "overmerge", "merged evts"
    );
    for dataset in &datasets {
        let mined = assign(&dataset.content);
        let s = score(&dataset.truth, &mined);
        let delta = baseline
            .as_ref()
            .and_then(|b| b.get(&dataset.name))
            .map(|old| {
                let d = (s.accuracy - old.accuracy) * 100.0;
                if d.abs() < REGRESSION_TOLERANCE * 100.0 {
                    "        =".to_string()
                } else {
                    format!("{d:>+8.1}pp")
                }
            })
            .unwrap_or_else(|| "      new".to_string());
        println!(
            "{:<13}{:>8.1}%{:>8}{:>10}{:>11}{:>11}{:>13}   {}",
            dataset.name,
            s.accuracy * 100.0,
            s.templates,
            s.clusters,
            s.oversplit,
            s.overmerged,
            s.overmerged_events,
            delta,
        );
        let diag = diagnostics(dataset, &mined);
        if !diag.is_empty() {
            details.push(format!("  {}:", dataset.name));
            details.extend(diag);
        }
        scores.insert(dataset.name.clone(), s);
    }

    let mean = scores.values().map(|s| s.accuracy).sum::<f64>() / scores.len() as f64;
    let merged_events: usize = scores.values().map(|s| s.overmerged_events).sum();
    println!(
        "\nmean accuracy {:.1}% over {} datasets; {} event(s) in over-merged templates",
        mean * 100.0,
        scores.len(),
        merged_events
    );
    if let Some(base) = &baseline {
        let base_mean = base.values().map(|s| s.accuracy).sum::<f64>() / base.len() as f64;
        println!(
            "baseline mean {:.1}% ({:+.1}pp)",
            base_mean * 100.0,
            (mean - base_mean) * 100.0
        );
    }
    println!("\nworst groupings per dataset:");
    for line in &details {
        println!("{line}");
    }

    if update {
        write_baseline(&scores);
        println!("\nbaseline written to {}", baseline_path().display());
        return;
    }

    // Regressions fail the run. Accuracy is the headline, but a jump in
    // over-merged events is called out separately: it can rise while accuracy
    // also rises, and it is the direction that loses information for good.
    let Some(base) = baseline else {
        println!("\nno baseline yet — run `just drain-accuracy-update` to record one",);
        return;
    };
    let mut regressions = Vec::new();
    for (name, s) in &scores {
        let Some(old) = base.get(name) else { continue };
        if s.accuracy < old.accuracy - REGRESSION_TOLERANCE {
            regressions.push(format!(
                "{name}: accuracy {:.1}% -> {:.1}%",
                old.accuracy * 100.0,
                s.accuracy * 100.0
            ));
        }
        if s.overmerged_events > old.overmerged_events {
            regressions.push(format!(
                "{name}: events in over-merged templates {} -> {}",
                old.overmerged_events, s.overmerged_events
            ));
        }
    }
    assert!(
        regressions.is_empty(),
        "drain accuracy regressed against dev/drain-accuracy-baseline.json:\n  {}\n\
         If the change is intended, re-record with `just drain-accuracy-update`.",
        regressions.join("\n  ")
    );
}
