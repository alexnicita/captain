#[path = "../../src/rust-harness/commit_subject_quality.rs"]
mod commit_subject_quality;

use commit_subject_quality::{
    deterministic_subject_from_files, has_informative_subject_scope, is_generic_subject,
    normalize_scope_token,
};

#[test]
fn normalize_scope_token_is_deterministic() {
    assert_eq!(
        normalize_scope_token(" Runtime Gate / Commit Quality "),
        "runtime-gate-commit-quality"
    );
    assert_eq!(normalize_scope_token("__src/coding.rs__"), "src-coding-rs");
}

#[test]
fn rejects_known_generic_template_subjects() {
    let subject = "build a generalizable patch flow — harness: coding cycle";
    assert!(is_generic_subject(subject));
}

#[test]
fn accepts_scoped_informative_subject_for_changed_src_files() {
    let changed = ["src/coding.rs", "src/code/diff.rs"];
    let subject = "fix(coding): enforce deterministic informative subjects";
    assert!(has_informative_subject_scope(subject, &changed));
}

#[test]
fn rejects_scope_mismatch_even_when_conventional() {
    let changed = ["src/coding.rs", "src/code/task.rs"];
    let subject = "fix(provider): tune endpoint fallback";
    assert!(!has_informative_subject_scope(subject, &changed));
}

#[test]
fn accepts_hyphen_underscore_equivalent_scope_tokens() {
    let changed = ["src/runtime_gate.rs", "src/coding.rs"];

    let subject_hyphen = "fix(runtime-gate): tighten deadline parsing";
    let subject_underscore = "fix(runtime_gate): tighten deadline parsing";

    assert!(has_informative_subject_scope(subject_hyphen, &changed));
    assert!(has_informative_subject_scope(subject_underscore, &changed));
}

#[test]
fn accepts_dotted_stem_scope_variant() {
    let changed = ["src/runtime_gate.v2.rs"];
    let subject = "fix(runtime-gate-v2): preserve parsing compatibility";
    assert!(has_informative_subject_scope(subject, &changed));
}

#[test]
fn deterministic_subject_names_primary_changed_files() {
    let changed = vec![
        "src/coding.rs".to_string(),
        "tests/rust-harness/commit_subject_quality_gate_v2.rs".to_string(),
    ];

    assert_eq!(
        deterministic_subject_from_files(&changed),
        "implement scoped code updates in src/coding.rs, tests/rust-harness/commit_subject_quality_gate_v2.rs"
    );
}

#[test]
fn deterministic_subject_prioritizes_src_when_generated_docs_are_also_staged() {
    let changed = vec![
        "captain/harnesses/rust-harness/RUNBOOK.md".to_string(),
        "captain/harnesses/rust-harness/toolsets/code/ROADMAP.md".to_string(),
        "captain/src/dogfood_smoke.rs".to_string(),
    ];

    let subject = deterministic_subject_from_files(&changed);

    assert!(
        subject.contains("dogfood_smoke.rs"),
        "subject should mention source scope, got {subject:?}"
    );
    assert!(
        subject.starts_with("implement scoped code updates"),
        "subject should classify mixed src/doc diffs as code work, got {subject:?}"
    );
    assert!(has_informative_subject_scope(
        &subject,
        &["captain/src/dogfood_smoke.rs"]
    ));
}
