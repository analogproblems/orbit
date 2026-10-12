use super::replies::*;
use super::ClaimScope;
use crate::types::ClaimKind;

fn one(lines: Vec<ReplyLine>) -> ReplyLine {
    lines.into_iter().next().expect("a reply line")
}

#[test]
fn orbit_grammar_fixed_with_ids_and_commit() {
    let l = one(exact_lines("Fixed F2 in 3f9c2a1"));
    assert_eq!(l.numbers, [2]);
    assert_eq!(l.kind, ClaimKind::Fixed);
    assert_eq!(l.commit.as_deref(), Some("3f9c2a1"));
    assert_eq!(l.scope, ClaimScope::Named);
}
#[test]
fn orbit_grammar_several_ids_bold_and_list_markers() {
    let l = one(exact_lines("- **Fixed F1, F3 in 3f9c2a1**"));
    assert_eq!(l.numbers, [1, 3]);
    assert_eq!(l.commit.as_deref(), Some("3f9c2a1"));
}
#[test]
fn orbit_grammar_declined_with_reason() {
    let l = one(exact_lines(
        "Declined F4: the caller in src/api.rs rejects an empty list, so this path can't be reached.",
    ));
    assert_eq!(l.numbers, [4]);
    assert_eq!(l.kind, ClaimKind::Declined);
    assert!(l.reason.unwrap().starts_with("the caller"));
}
#[test]
fn orbit_grammar_thread_forms() {
    assert_eq!(
        one(exact_lines("Fixed in abc1234")).scope,
        ClaimScope::Thread
    );
    let l = one(exact_lines("Declined: not reachable"));
    assert_eq!(l.scope, ClaimScope::Thread);
    assert_eq!(l.reason.as_deref(), Some("not reachable"));
}
#[test]
fn fixed_the_tests_names_no_finding() {
    assert!(reply_lines("Fixed the tests").is_empty());
}
#[test]
fn older_finding_is_fixed_in_a_commit() {
    let l = one(legacy_lines(
        "F1 (a stale settings-only button could reconnect a repository) is fixed in f2336fc.",
    ));
    assert_eq!(l.numbers, [1]);
    assert_eq!(l.commit.as_deref(), Some("f2336fc"));
    assert_eq!(l.scope, ClaimScope::Named);
}
#[test]
fn older_all_findings_fixed_in_a_commit() {
    assert_eq!(
        one(legacy_lines("All three findings are fixed in 1e8b66f.")).scope,
        ClaimScope::All
    );
    assert_eq!(
        one(legacy_lines("Both findings are fixed in 0ab9947."))
            .commit
            .as_deref(),
        Some("0ab9947")
    );
    assert_eq!(
        one(legacy_lines("Thanks, both are fixed in 2b65d93.")).scope,
        ClaimScope::All
    );
}
#[test]
fn older_per_finding_lines_take_the_comments_commit() {
    let lines = legacy_lines(
        "Addressed Milo's review of `9aaf81e` in `3a235dc`.\n\n**F1 (P2), fixed.** Rules now use snake_case.\n\n**F2 (P1), fixed.** A fix only runs while still_mine holds.",
    );
    assert_eq!(lines[0].scope, ClaimScope::All);
    assert_eq!(lines[0].commit.as_deref(), Some("3a235dc"));
    let rest: Vec<(u32, Option<&str>)> = lines[1..]
        .iter()
        .map(|l| (l.numbers[0], l.commit.as_deref()))
        .collect();
    assert_eq!(rest, [(1, Some("3a235dc")), (2, Some("3a235dc"))]);
}
#[test]
fn older_f1_fixed_in_a_commit() {
    let l = one(reply_lines(
        "**F1 fixed in 0db1179.** `save_repository` now runs the whole save.",
    ));
    assert_eq!(l.numbers, [1]);
    assert_eq!(l.commit.as_deref(), Some("0db1179"));
}
#[test]
fn older_declines_and_lines_that_say_not_fixed() {
    let l = one(legacy_lines("F3 is intentional: the cache is per window."));
    assert_eq!(l.kind, ClaimKind::Declined);
    assert_eq!(l.numbers, [3]);
    assert!(legacy_lines("F2 is not fixed yet.").is_empty());
}
