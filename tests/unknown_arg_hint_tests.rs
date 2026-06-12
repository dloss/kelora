// Tests for curated "did you mean" hints on unknown arguments.
//
// kelora replaces clap's edit-distance suggestion with intent-based guidance for a
// small, curated set of flags borrowed from other tools (e.g. --sort, --grep). These
// are NOT aliases: the flags stay unknown and exit 2, so no namespace is reserved.

mod common;
use common::run_kelora_with_input;

#[test]
fn curated_filter_synonyms_point_to_filter() {
    for flag in ["--where", "--grep", "--match"] {
        let (_out, err, code) = run_kelora_with_input(&["-f", "json", flag, "x"], "{}\n");
        assert_eq!(code, 2, "{flag} should exit 2");
        assert!(
            err.contains("--filter"),
            "{flag} hint should mention --filter, got:\n{err}"
        );
        assert!(
            err.contains(&format!("unexpected argument '{flag}'")),
            "{flag} should still report an unexpected argument, got:\n{err}"
        );
    }
}

#[test]
fn curated_ranking_synonyms_point_to_track_top_by() {
    for flag in ["--sort", "--top", "--rank"] {
        let (_out, err, code) = run_kelora_with_input(&["-f", "json", flag, "x"], "{}\n");
        assert_eq!(code, 2, "{flag} should exit 2");
        assert!(
            err.contains("track_top_by"),
            "{flag} hint should mention track_top_by, got:\n{err}"
        );
    }
}

#[test]
fn curated_aggregation_synonyms_point_to_track_count() {
    for flag in ["--count", "--group-by", "--uniq"] {
        let (_out, err, code) = run_kelora_with_input(&["-f", "json", flag], "{}\n");
        assert_eq!(code, 2, "{flag} should exit 2");
        assert!(
            err.contains("track_count"),
            "{flag} hint should mention track_count, got:\n{err}"
        );
    }
}

#[test]
fn curated_flags_are_not_real_aliases() {
    // A curated flag must NOT silently behave like its target; it stays unknown.
    let (out, err, code) = run_kelora_with_input(&["-f", "json", "--grep", "x"], "{}\n");
    assert_eq!(code, 2);
    assert!(
        out.is_empty(),
        "no events should be emitted, got stdout:\n{out}"
    );
    assert!(err.contains("unexpected argument"));
}

#[test]
fn genuine_typo_falls_back_to_clap_suggestion() {
    // Not in the curated set: clap's edit-distance suggestion is correct here and
    // must be preserved (we only override the curated names).
    let (_out, err, code) = run_kelora_with_input(&["-f", "json", "--filer", "x"], "{}\n");
    assert_eq!(code, 2);
    assert!(
        err.contains("--filter"),
        "clap should suggest --filter for --filer, got:\n{err}"
    );
    assert!(
        !err.contains("hint:"),
        "non-curated typo should not get a curated hint, got:\n{err}"
    );
}
