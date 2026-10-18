//! From one stored pull request to its history: who said what, the
//! reviewer's rounds, the builder's claims, each finding's way to a ruling,
//! some numbers, and the flags worth a glance.
use super::*;
use std::collections::HashMap;

/// A merge within this many minutes of a clean last round counts as the
/// loop's merge.
const MERGE_WINDOW_MINUTES: i64 = 45;

pub fn derive(stored: &StoredPr) -> History {
    let pr = &stored.pr;
    let comments = comment_refs(&stored.comments);
    let rounds = rounds(&comments);
    let claims = claims(&comments);
    let merged_at = pr.merged_at.clone().filter(|_| pr.merged);
    let trajectories = trajectories(&rounds, &claims, merged_at.as_deref());
    let metrics = metrics(pr, &rounds, &trajectories);
    let flags = flags(pr, &rounds, &trajectories);
    History {
        repo: stored.repo.clone(),
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        state: pr.state.clone(),
        merged: pr.merged,
        draft: pr.draft,
        author: pr.author.clone(),
        created_at: pr.created_at.clone(),
        updated_at: pr.updated_at.clone(),
        closed_at: pr.closed_at.clone(),
        merged_at,
        head_ref: pr.head_ref.clone(),
        base_ref: pr.base_ref.clone(),
        additions: pr.additions,
        deletions: pr.deletions,
        changed_files: pr.changed_files,
        fetched_at: stored.fetched_at.clone(),
        narrative: narrative::parse(&pr.body, &pr.title),
        rounds,
        claims,
        trajectories,
        metrics,
        flags,
        writeup: None,
        comments,
        commits: stored.commits.clone(),
        files: stored.files.clone(),
    }
}

/// The head commit a reviewer's comment names.
fn reviewed_head(body: &str) -> Option<String> {
    if orbit::is_review(body) {
        orbit::head(body)
    } else {
        milo::head(body)
    }
}

/// The texts a comment has had that each count: a reviewer's comment edited
/// in place for a new head keeps one text per head it reviewed, at the time
/// it first named that head. Any other comment is its latest text.
fn versions(c: &StoredComment, role: Role) -> Vec<(String, String)> {
    let latest = vec![(c.body.clone(), c.created_at.clone())];
    if role != Role::Reviewer || c.revisions.is_empty() {
        return latest;
    }
    // Oldest first, each with when it became the comment's text.
    let mut all: Vec<(String, String)> = c
        .revisions
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let at = if i == 0 { &c.created_at } else { &r.updated_at };
            (r.body.clone(), at.clone())
        })
        .collect();
    all.push((c.body.clone(), c.updated_at.clone()));
    let mut runs: Vec<(Option<String>, String, String)> = vec![];
    for (body, at) in all {
        let head = reviewed_head(&body);
        match runs.last_mut() {
            Some((h, text, _)) if *h == head => *text = body,
            _ => runs.push((head, body, at)),
        }
    }
    if runs.len() == 1 {
        return latest;
    }
    runs.into_iter().map(|(_, body, at)| (body, at)).collect()
}

pub(super) fn comment_refs(stored: &[StoredComment]) -> Vec<CommentRef> {
    let mut out = vec![];
    for c in stored {
        // An empty review is only an approval or a container for comments.
        if c.kind == CommentKind::Review && c.body.trim().is_empty() {
            continue;
        }
        let role = roles::infer(&c.body, &c.author, &c.association);
        let versions = versions(c, role);
        let last = versions.len() - 1;
        // Only the reviewer's own inline comments can carry a finding.
        let may_review = roles::may_review(&c.author, &c.association);
        for (i, (body, created_at)) in versions.into_iter().enumerate() {
            let finding_marker = (c.kind == CommentKind::ReviewComment && may_review)
                .then(|| orbit::finding_marker(&body).or_else(|| legacy_marker(c, &body)))
                .flatten();
            out.push(CommentRef {
                id: c.id,
                kind: c.kind,
                author: c.author.clone(),
                role: roles::infer(&body, &c.author, &c.association),
                created_at,
                url: c.url.clone(),
                body,
                path: c.path.clone(),
                line: c.line,
                start_line: c.start_line,
                in_reply_to: c.in_reply_to,
                review_id: c.review_id,
                finding_marker,
                edit: (i < last).then_some(i),
            });
        }
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    out
}
/// Milo's inline finding: the first comment of a thread whose first line is
/// a finding heading.
fn legacy_marker(c: &StoredComment, body: &str) -> Option<String> {
    if c.in_reply_to.is_some() {
        return None;
    }
    let h = milo::heading(body.trim().lines().next().unwrap_or(""))?;
    h.ruling.is_none().then(|| format!("rc{}", c.id))
}

/// Milo's finding posted as a comment on the changed lines of its review.
fn legacy_inline(ic: &CommentRef, marker: &str) -> Option<Finding> {
    let mut lines = ic.body.trim().lines();
    let h = milo::heading(lines.next().unwrap_or(""))?;
    if h.ruling.is_some() {
        return None;
    }
    let rest: Vec<&str> = lines.collect();
    let rest = rest.join("\n");
    let mut f = Finding::new(
        marker.to_string(),
        if h.title.is_empty() {
            "(untitled)".into()
        } else {
            h.title
        },
        h.priority,
    );
    f.short_id = h.number.map(|n| format!("F{n}"));
    f.number = h.number;
    f.blocking = Some(h.priority <= 2);
    if let Some(path) = &ic.path {
        let line = ic.line.unwrap_or(0);
        f.locations.push(Place {
            path: Some(path.clone()),
            start_line: line,
            end_line: line,
        });
    }
    f.problem = rest.split("\n\n").next().unwrap_or("").trim().to_string();
    if rest.to_lowercase().contains("reproduced") || ic.body.to_lowercase().contains("reproduced") {
        f.confirmed = Some(true);
    }
    f.inline = Some(true);
    Some(f)
}

/// The reviewer's comments as rounds, in time order.
fn rounds(comments: &[CommentRef]) -> Vec<Round> {
    let mut inline: Vec<Finding> = vec![];
    for c in comments {
        if c.kind != CommentKind::ReviewComment || c.finding_marker.is_none() {
            continue;
        }
        if let Some(f) = orbit::inline_finding(c) {
            inline.retain(|i| i.id != f.id);
            inline.push(f);
        }
    }
    let mut rounds: Vec<Round> = vec![];
    for c in comments {
        if c.role != Role::Reviewer || c.kind == CommentKind::ReviewComment {
            continue;
        }
        if let Some(mut round) = orbit::parse_review(c) {
            // Without the hidden data, which is left out for size, the text
            // lists only the findings that aren't inline; each inline comment
            // still carries its own. Finding IDs start with their review's.
            let from_text = !round.findings.iter().any(|f| f.inline == Some(true));
            if let Some(prefix) = round.review_id.as_ref().map(|id| format!("{id}-")) {
                if from_text {
                    round
                        .findings
                        .extend(inline.iter().filter(|f| f.id.starts_with(&prefix)).cloned());
                }
            }
            rounds.push(round);
        } else if let Some(mut round) = milo::parse_review(c) {
            if c.kind == CommentKind::Review {
                // Early reviews put each finding in its own inline comment.
                for ic in comments {
                    let Some(marker) = ic.finding_marker.as_deref() else {
                        continue;
                    };
                    if ic.kind != CommentKind::ReviewComment
                        || ic.review_id != Some(c.id)
                        || !marker.starts_with("rc")
                    {
                        continue;
                    }
                    if let Some(f) = legacy_inline(ic, marker) {
                        round.findings.push(f);
                    }
                }
                if round.verdict == RoundVerdict::Unknown && !round.findings.is_empty() {
                    round.verdict = if round.findings.iter().any(|f| f.blocking == Some(true)) {
                        RoundVerdict::ChangesRequested
                    } else {
                        RoundVerdict::Clean
                    };
                }
                if round.verdict == RoundVerdict::ChangesRequested {
                    round.blocking = Some(
                        round
                            .findings
                            .iter()
                            .filter(|f| f.blocking != Some(false))
                            .count() as u32,
                    );
                }
            }
            rounds.push(round);
        }
    }
    rounds.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    // Milo listed every open finding again each round: the first listing
    // opens the finding, and later ones rule that it is still open.
    let mut seen: HashMap<String, ()> = HashMap::new();
    for r in rounds.iter_mut().filter(|r| r.format == Format::Milo) {
        let mut fresh = vec![];
        for f in std::mem::take(&mut r.findings) {
            let key = f.short_id.clone().unwrap_or_else(|| f.id.clone());
            if f.short_id.is_some() && seen.contains_key(&key) {
                let ruled = r
                    .earlier_findings
                    .iter()
                    .any(|e| e.short_id.as_ref().unwrap_or(&e.id) == &key);
                if !ruled {
                    r.earlier_findings.push(Earlier {
                        id: key.clone(),
                        short_id: f.short_id.clone(),
                        title: f.title.clone(),
                        priority: Some(f.priority),
                        status: Some(PriorStatus::StillOpen),
                        note: Some("Listed again in this round.".into()),
                        author_said: None,
                    });
                }
            } else {
                seen.insert(key, ());
                fresh.push(f);
            }
        }
        r.findings = fresh;
        if r.verdict == RoundVerdict::ChangesRequested && r.blocking.is_some() {
            let open = r
                .earlier_findings
                .iter()
                .filter(|e| {
                    matches!(
                        e.status,
                        Some(PriorStatus::StillOpen | PriorStatus::DeclinedDisputed)
                    )
                })
                .count();
            let new = r
                .findings
                .iter()
                .filter(|f| f.blocking != Some(false))
                .count();
            r.blocking = Some((new + open) as u32);
        }
    }
    for (i, r) in rounds.iter_mut().enumerate() {
        if r.round.is_none() {
            r.round = Some(i as u32 + 1);
        }
    }
    rounds
}

/// Every reply line naming findings, including replies in a finding's own
/// thread.
fn claims(comments: &[CommentRef]) -> Vec<Claim> {
    let roots: HashMap<u64, &str> = comments
        .iter()
        .filter_map(|c| Some((c.id, c.finding_marker.as_deref()?)))
        .collect();
    let mut out = vec![];
    for c in comments.iter().filter(|c| c.role != Role::Reviewer) {
        let thread = c
            .in_reply_to
            .and_then(|id| roots.get(&id))
            .map(|s| s.to_string());
        for line in replies::reply_lines(&c.body) {
            out.push(Claim {
                comment_id: c.id,
                url: c.url.clone(),
                created_at: c.created_at.clone(),
                author: c.author.clone(),
                numbers: line.numbers,
                kind: line.kind,
                commit: line.commit,
                reason: line.reason,
                scope: if line.scope == ClaimScope::Thread && thread.is_none() {
                    ClaimScope::All
                } else {
                    line.scope
                },
                thread_finding_id: thread.clone(),
            });
        }
    }
    out
}

/// Follows each finding from the round that raised it to its last ruling.
fn trajectories(rounds: &[Round], claims: &[Claim], merged_at: Option<&str>) -> Vec<Trajectory> {
    let mut sorted: Vec<&Claim> = claims.iter().collect();
    sorted.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let mut out = vec![];
    for opened in rounds {
        for f in &opened.findings {
            let opened_at = &opened.created_at;
            let mut rulings: Vec<Ruling> = vec![];
            for r in rounds.iter().filter(|r| r.created_at > *opened_at) {
                let earlier = r.earlier_findings.iter().find(|e| {
                    e.id == f.id
                        || f.short_id.as_ref().is_some_and(|s| {
                            e.short_id.as_ref().unwrap_or(&e.id).eq_ignore_ascii_case(s)
                        })
                });
                if let Some((e, status)) = earlier.and_then(|e| Some((e, e.status?))) {
                    rulings.push(Ruling {
                        round: r.round.unwrap_or(0),
                        at: r.created_at.clone(),
                        status,
                        note: e.note.clone(),
                    });
                }
            }
            // The builder's latest word. A line naming the finding, or a
            // reply in its thread, always counts; an "all fixed" line counts
            // only while the finding has no claim yet, or was ruled still
            // open after its last claim.
            let mut claim: Option<&Claim> = None;
            for c in sorted.iter().filter(|c| c.created_at >= *opened_at) {
                let named = f.number.is_some_and(|n| c.numbers.contains(&n));
                let thread = c.scope == ClaimScope::Thread
                    && c.thread_finding_id.as_deref() == Some(f.id.as_str());
                if named || thread {
                    claim = Some(c);
                    continue;
                }
                if c.scope != ClaimScope::All {
                    continue;
                }
                let fixed_before = rulings
                    .iter()
                    .any(|r| r.at < c.created_at && r.status == PriorStatus::Fixed);
                let reopened = claim.is_some_and(|earlier| {
                    rulings.iter().any(|r| {
                        r.at > earlier.created_at
                            && r.at < c.created_at
                            && r.status == PriorStatus::StillOpen
                    })
                });
                if !fixed_before && (claim.is_none() || reopened) {
                    claim = Some(c);
                }
            }
            let (final_status, cleared) = standing(&rulings, rounds, opened_at, None);
            let status_at_merge = merged_at
                .filter(|m| opened_at.as_str() <= *m)
                .map(|m| standing(&rulings, rounds, opened_at, Some(m)).0);
            if let Some(clean) = cleared {
                rulings.push(Ruling {
                    round: clean.round.unwrap_or(0),
                    at: clean.created_at.clone(),
                    status: PriorStatus::Fixed,
                    note: Some("Clean verdict with no further mention (inferred).".into()),
                });
            }
            out.push(Trajectory {
                finding_id: f.id.clone(),
                short_id: f.short_id.clone(),
                opened_round: opened.round.unwrap_or(0),
                opened_at: opened_at.clone(),
                priority: f.priority,
                blocking: f.blocking,
                category: f.category,
                title: f.title.clone(),
                minutes_to_claim: claim
                    .filter(|c| c.kind == ClaimKind::Fixed)
                    .and_then(|c| minutes_between(opened_at, &c.created_at)),
                claim: claim.cloned(),
                rulings,
                final_status,
                open_at_merge: matches!(
                    status_at_merge,
                    Some(
                        FinalStatus::Open | FinalStatus::StillOpen | FinalStatus::DeclinedDisputed
                    )
                ),
                status_at_merge,
            });
        }
    }
    out
}

/// Where a finding stood by `until` (after every round when None): its last
/// ruling, or fixed when a later clean round stopped raising it, with that
/// round.
fn standing<'a>(
    rulings: &[Ruling],
    rounds: &'a [Round],
    opened_at: &str,
    until: Option<&str>,
) -> (FinalStatus, Option<&'a Round>) {
    let by = |at: &str| until.is_none_or(|u| at <= u);
    let last = rulings.iter().rfind(|r| by(&r.at));
    let status = last.map_or(FinalStatus::Open, |r| r.status.into());
    if !matches!(status, FinalStatus::Open | FinalStatus::StillOpen) {
        return (status, None);
    }
    // A later clean round that doesn't mention it means the reviewer
    // stopped raising it.
    let clean = rounds.iter().find(|r| {
        r.created_at.as_str() > opened_at
            && by(&r.created_at)
            && r.verdict == RoundVerdict::Clean
            && last.is_none_or(|l| r.created_at > l.at)
    });
    match clean {
        Some(round) => (FinalStatus::Fixed, Some(round)),
        None => (status, None),
    }
}

fn metrics(pr: &PrInfo, rounds: &[Round], trajectories: &[Trajectory]) -> Metrics {
    let merged_at = pr.merged_at.as_deref().filter(|_| pr.merged);
    let first_clean = rounds.iter().find(|r| r.verdict == RoundVerdict::Clean);
    let last = rounds.last();
    let merged_by_loop = merged_at.map(|merged| {
        let last_before = rounds
            .iter()
            .rev()
            .find(|r| r.created_at.as_str() <= merged);
        last_before.is_some_and(|last| {
            last.verdict == RoundVerdict::Clean
                && minutes_between(&last.created_at, merged)
                    .is_some_and(|gap| (0..=MERGE_WINDOW_MINUTES).contains(&gap))
        })
    });
    let count = |s: FinalStatus| trajectories.iter().filter(|t| t.final_status == s).count() as u32;
    let unverified: Vec<&str> = rounds
        .iter()
        .flat_map(|r| &r.findings)
        .filter(|f| f.confirmed == Some(false))
        .map(|f| f.id.as_str())
        .collect();
    Metrics {
        rounds: rounds.len() as u32,
        findings: trajectories.len() as u32,
        blocking_findings: trajectories
            .iter()
            .filter(|t| t.blocking == Some(true))
            .count() as u32,
        fixed: count(FinalStatus::Fixed),
        declined: count(FinalStatus::DeclinedAccepted) + count(FinalStatus::DeclinedDisputed),
        disputed: count(FinalStatus::DeclinedDisputed),
        open_at_merge: trajectories.iter().filter(|t| t.open_at_merge).count() as u32,
        minutes_to_clean: first_clean.and_then(|r| minutes_between(&pr.created_at, &r.created_at)),
        minutes_to_merge: merged_at.and_then(|m| minutes_between(&pr.created_at, m)),
        first_verdict: rounds.first().map(|r| r.verdict),
        final_verdict: last.map(|r| r.verdict),
        merged_by_loop,
        verification_gaps: last.map_or(0, |r| r.not_checked.len() as u32),
        likely_not_verified: trajectories
            .iter()
            .filter(|t| unverified.contains(&t.finding_id.as_str()))
            .count() as u32,
    }
}

fn flags(pr: &PrInfo, rounds: &[Round], trajectories: &[Trajectory]) -> Vec<Flag> {
    let merged = pr.merged;
    let merged_at = pr.merged_at.as_deref().filter(|_| merged);
    let url = Some(pr.url.clone()).filter(|u| !u.is_empty());
    let mut out = vec![];
    for t in trajectories {
        let label = t.short_id.as_deref().unwrap_or(&t.title);
        let disputed_at_merge = t.status_at_merge == Some(FinalStatus::DeclinedDisputed);
        let disputed_now = t.final_status == FinalStatus::DeclinedDisputed;
        // A decline disputed at the merge and still disputed has its own
        // flag below; once the dispute is settled, this one keeps the merge.
        if t.open_at_merge && !(disputed_at_merge && disputed_now) {
            out.push(Flag {
                kind: FlagKind::MergedWithOpenFindings,
                attention: Glance::Act,
                text: if disputed_at_merge {
                    format!(
                        "Merged while the reviewer disputed the decline of {label} ({}).",
                        t.title
                    )
                } else {
                    format!("Merged while {label} ({}) was still open.", t.title)
                },
                finding_id: Some(t.finding_id.clone()),
                round: Some(t.opened_round),
                url: url.clone(),
            });
        }
        if disputed_now {
            out.push(Flag {
                kind: FlagKind::DisputedDecline,
                attention: Glance::Act,
                text: format!(
                    "The builder declined {label} ({}) and the reviewer still thinks it is a real problem.",
                    t.title
                ),
                finding_id: Some(t.finding_id.clone()),
                round: Some(t.opened_round),
                url: url.clone(),
            });
        }
        let still_open = t
            .rulings
            .iter()
            .filter(|r| r.status == PriorStatus::StillOpen)
            .count();
        if t.blocking == Some(true) && still_open >= 2 {
            out.push(Flag {
                kind: FlagKind::RepeatBlocking,
                attention: Glance::Look,
                text: format!(
                    "{label} ({}) stayed open across {} rounds.",
                    t.title,
                    still_open + 1
                ),
                finding_id: Some(t.finding_id.clone()),
                round: Some(t.opened_round),
                url: url.clone(),
            });
        }
    }
    let reviewed_by_merge = rounds
        .iter()
        .any(|r| merged_at.is_none_or(|m| r.created_at.as_str() <= m));
    if merged && !reviewed_by_merge {
        out.push(Flag {
            kind: FlagKind::NoReviewBeforeMerge,
            attention: Glance::Look,
            text: "Merged before any reviewer round Orbit can see.".into(),
            finding_id: None,
            round: None,
            url: url.clone(),
        });
    }
    if let Some(last) = rounds.last() {
        if last.format == Format::Orbit && !last.not_checked.is_empty() {
            out.push(Flag {
                kind: FlagKind::ReviewerCouldNotCheck,
                attention: Glance::Look,
                text: format!(
                    "The reviewer could not check: {}",
                    last.not_checked.join("; ")
                ),
                finding_id: None,
                round: last.round,
                url: Some(last.url.clone()),
            });
        }
        if last.completed == Some(false) {
            out.push(Flag {
                kind: FlagKind::ReviewerCouldNotCheck,
                attention: Glance::Look,
                text: "The reviewer said it could not read or check the whole change.".into(),
                finding_id: None,
                round: last.round,
                url: Some(last.url.clone()),
            });
        }
    }
    if merged {
        for r in rounds {
            for f in r
                .findings
                .iter()
                .filter(|f| f.blocking == Some(true) && f.confirmed == Some(false))
            {
                out.push(Flag {
                    kind: FlagKind::LikelyNotVerified,
                    attention: Glance::Look,
                    text: format!(
                        "{} was marked likely but not verified by the reviewer.",
                        f.short_id.as_deref().unwrap_or(&f.title)
                    ),
                    finding_id: Some(f.id.clone()),
                    round: r.round,
                    url: Some(r.url.clone()),
                });
            }
        }
    }
    out
}
