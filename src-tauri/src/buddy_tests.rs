use super::*;
use crate::types::tests::pr;
use serde_json::json;

fn snapshot() -> (Snapshot, PullRequest) {
    let p = pr();
    let mut s = Snapshot::default();
    s.settings.automatic = true;
    s.settings.auto_post = true;
    s.settings.repositories.push(Repository {
        name: "a/b".into(),
        path: "/tmp/a-b".into(),
        automatic: true,
        setup_command: String::new(),
        network_access: false,
        round_limit: 4,
        approval_minutes: 0,
        keep_updated: false,
        provider: Default::default(),
    });
    s.prs.push(p.clone());
    (s, p)
}
/// An automatic round on `p`'s head, created at `minute`.
fn round(p: &PullRequest, minute: u32) -> Review {
    let mut r = Review::new(p, &Settings::default(), true);
    r.id = format!("run-{minute}");
    r.created_at = format!("2026-01-01T00:{minute:02}:00.000Z");
    r.status = "reviewing".into();
    r
}
/// Marks a round posted with `blocking` blocking findings.
fn finish(r: &mut Review, blocking: u32) {
    r.status = "posted".into();
    r.findings = Some(vec![]);
    r.blocking = Some(blocking);
    r.orbit_verdict = Some(if blocking == 0 {
        Verdict::Clean
    } else {
        Verdict::ChangesRequested
    });
}
/// What the engine does on each flush: announce new items, then let Bit
/// look. Returns the notes written this time.
fn step(s: &mut Snapshot) -> Vec<(NoteKind, String)> {
    let before = s.buddy_feed.next_id;
    let (notices, _) = crate::attention::announce(s);
    let fresh: Vec<Attention> = notices.into_iter().map(|n| n.item).collect();
    observe(s, &fresh);
    s.buddy_feed
        .notes
        .iter()
        .filter(|n| n.id >= before)
        .map(|n| (n.kind, n.text.clone()))
        .collect()
}
fn seeded() -> (Snapshot, PullRequest) {
    let (mut s, p) = snapshot();
    assert!(step(&mut s).is_empty());
    assert!(s.buddy_feed.seeded);
    (s, p)
}

#[test]
fn the_first_look_records_loops_without_notes() {
    let (mut s, p) = snapshot();
    let mut r = round(&p, 1);
    finish(&mut r, 2);
    s.reviews.push(r);
    assert!(step(&mut s).is_empty());
    assert_eq!(s.buddy_feed.loops.len(), 1);
    assert_eq!(s.buddy_feed.loops[0].finished, 1);
    assert!(step(&mut s).is_empty());
}
#[test]
fn each_change_in_a_loop_writes_one_note() {
    let (mut s, mut p) = seeded();
    s.reviews.push(round(&p, 1));
    assert_eq!(
        step(&mut s),
        [(
            NoteKind::RoundStarted,
            "On it! Round 1 of b#1 is underway.".to_string()
        )]
    );
    assert!(step(&mut s).is_empty(), "nothing changed, nothing written");
    finish(&mut s.reviews[0], 2);
    assert_eq!(
        step(&mut s),
        [(
            NoteKind::RoundFinished,
            "Round 1 of b#1 found 2 blocking findings. Over to the author!".to_string()
        )]
    );
    // The author pushes; round 2 comes back clean.
    p.head_sha = "c".repeat(40);
    s.prs[0] = p.clone();
    assert!(step(&mut s).is_empty(), "waiting for the next round");
    s.reviews.push(round(&p, 2));
    assert_eq!(step(&mut s)[0].1, "On it! Round 2 of b#1 is underway.");
    finish(&mut s.reviews[1], 0);
    assert_eq!(
        step(&mut s),
        [(
            NoteKind::Clean,
            "b#1 is clean after 2 rounds! 🎉 2 → 0".to_string()
        )]
    );
    assert_eq!(
        s.buddy_feed.notes.last().unwrap().review_id.as_deref(),
        Some("run-2")
    );
    // Paused, resumed, and given more rounds.
    s.loops.push(LoopControl {
        pr_id: p.id.clone(),
        stopped: true,
        extra_rounds: 0,
    });
    assert_eq!(
        step(&mut s),
        [(
            NoteKind::Stopped,
            "Pausing the loop on b#1. I'll be here.".to_string()
        )]
    );
    s.loops[0].stopped = false;
    assert_eq!(
        step(&mut s),
        [(NoteKind::Continued, "Back to work on b#1!".to_string())]
    );
    s.loops[0].extra_rounds = 1;
    assert_eq!(step(&mut s)[0].0, NoteKind::Continued);
    assert!(step(&mut s).is_empty());
}
#[test]
fn a_failed_round_says_so_and_its_needs_you_item_follows() {
    let (mut s, p) = seeded();
    s.reviews.push(round(&p, 1));
    step(&mut s);
    s.reviews[0].status = "failed".into();
    s.reviews[0].error = Some("Codex exited.".into());
    assert_eq!(
        step(&mut s),
        [
            (
                NoteKind::RoundFinished,
                "Oh no, round 1 of b#1 didn't finish.".to_string()
            ),
            (
                NoteKind::NeedsYou,
                "Could you take a look at b#1? Codex exited.".to_string()
            ),
        ]
    );
    let note = s.buddy_feed.notes.last().unwrap();
    assert_eq!(note.pr_id.as_deref(), Some("a/b#1"));
    assert_eq!(note.review_id.as_deref(), Some("run-1"));
}
#[test]
fn a_follow_up_chat_on_a_finished_round_repeats_nothing() {
    // A follow-up sets the round running until it ends, then restores the
    // round's outcome, whether the chat answered, failed or was interrupted.
    let endings = [None, Some("The follow-up failed: boom".to_string())];
    for (status, blocking, kind) in [
        ("posted", 0, NoteKind::Clean),
        ("posted", 2, NoteKind::RoundFinished),
        ("failed", 0, NoteKind::RoundFinished),
    ] {
        let (mut s, p) = seeded();
        s.reviews.push(round(&p, 1));
        step(&mut s);
        finish(&mut s.reviews[0], blocking);
        s.reviews[0].status = status.into();
        let finished = step(&mut s);
        assert_eq!(finished[0].0, kind, "{status} {blocking}");
        for ending in endings.iter().chain(&endings) {
            let r = &mut s.reviews[0];
            r.before_follow_up = Some(crate::types::Outcome {
                status: std::mem::replace(&mut r.status, "reviewing".into()),
                error: r.error.take(),
            });
            r.is_review = false;
            let during = step(&mut s);
            assert!(s.reviews[0].end_follow_up(ending.clone()));
            let after = step(&mut s);
            for (kind, text) in during.iter().chain(&after) {
                assert!(
                    !matches!(
                        kind,
                        NoteKind::RoundStarted | NoteKind::RoundFinished | NoteKind::Clean
                    ),
                    "{status} {blocking} {ending:?}: {text}"
                );
            }
            if status == "posted" {
                assert!(during.is_empty() && after.is_empty(), "{after:?}");
            }
        }
    }
}
#[test]
fn each_new_needs_you_item_writes_one_note() {
    let (mut s, p) = seeded();
    s.reviews.push(round(&p, 1));
    step(&mut s);
    s.reviews[0].status = "awaitingApproval".into();
    s.approvals.push(Approval {
        id: json!(7),
        review_id: "run-1".into(),
        method: "item/commandExecution/requestApproval".into(),
        params: json!({}),
        requested_at: String::new(),
    });
    assert_eq!(
        step(&mut s),
        [(
            NoteKind::NeedsYou,
            "Could you take a look at b#1? The reviewer asks to run a command.".to_string()
        )]
    );
    assert!(step(&mut s).is_empty());
    // A workspace item has no PR, but keeps its repository.
    s.sync_failures.push(SyncFailure {
        repo: "a/b".into(),
        count: crate::attention::SYNC_FAILURES,
        error: "timeout".into(),
    });
    let written = step(&mut s);
    assert_eq!(written.len(), 1);
    assert_eq!(
        written[0].1,
        "Heads up! a/b failed to sync 3 times in a row: timeout"
    );
    let note = s.buddy_feed.notes.last().unwrap();
    assert_eq!(
        (note.pr_id.as_deref(), note.repo.as_deref()),
        (None, Some("a/b"))
    );
}
#[test]
fn a_restart_neither_loses_nor_repeats_notes() {
    let (mut s, p) = seeded();
    s.reviews.push(round(&p, 1));
    step(&mut s);
    s.approvals.push(Approval {
        id: json!(1),
        review_id: "run-1".into(),
        method: "item/fileChange/requestApproval".into(),
        params: json!({}),
        requested_at: String::new(),
    });
    assert_eq!(step(&mut s).len(), 1);
    let saved = serde_json::to_vec(&s).unwrap();
    let mut restored: Snapshot = serde_json::from_slice(&saved).unwrap();
    assert_eq!(restored.buddy_feed.notes, s.buddy_feed.notes);
    assert!(step(&mut restored).is_empty());
}
#[test]
fn the_feed_keeps_the_last_fifty_notes() {
    let (mut s, p) = snapshot();
    let mut r = round(&p, 1);
    finish(&mut r, 1);
    s.reviews.push(r);
    step(&mut s);
    s.loops.push(LoopControl {
        pr_id: p.id.clone(),
        stopped: false,
        extra_rounds: 0,
    });
    for _ in 0..30 {
        s.loops[0].stopped = !s.loops[0].stopped;
        assert_eq!(step(&mut s).len(), 1);
    }
    assert_eq!(s.buddy_feed.notes.len(), 30);
    for _ in 0..30 {
        s.loops[0].stopped = !s.loops[0].stopped;
        step(&mut s);
    }
    assert_eq!(s.buddy_feed.notes.len(), MAX_NOTES);
    let ids: Vec<u64> = s.buddy_feed.notes.iter().map(|n| n.id).collect();
    assert_eq!(ids.first(), Some(&10));
    assert_eq!(ids.last(), Some(&59));
}
#[test]
fn with_the_buddy_off_nothing_is_written_and_nothing_is_saved_up() {
    let (mut s, p) = seeded();
    s.settings.buddy.enabled = false;
    s.reviews.push(round(&p, 1));
    assert!(step(&mut s).is_empty());
    finish(&mut s.reviews[0], 1);
    assert!(step(&mut s).is_empty());
    assert!(s.buddy_feed.notes.is_empty());
    // Turning it back on doesn't replay what happened meanwhile.
    s.settings.buddy.enabled = true;
    assert!(step(&mut s).is_empty());
}
#[test]
fn notes_use_the_chosen_tone() {
    let (mut s, p) = seeded();
    s.settings.buddy.tone = "dry".into();
    s.reviews.push(round(&p, 1));
    assert_eq!(step(&mut s)[0].1, "Round 1 on b#1. Here we go again.");
    s.settings.buddy.tone = "calm".into();
    finish(&mut s.reviews[0], 1);
    assert_eq!(
        step(&mut s)[0].1,
        "Round 1 on b#1 finished with 1 blocking finding."
    );
}
#[test]
fn every_tone_has_a_line_for_every_kind_of_note() {
    let kinds = [
        NoteKind::RoundStarted,
        NoteKind::RoundFinished,
        NoteKind::Clean,
        NoteKind::NeedsYou,
        NoteKind::Stopped,
        NoteKind::Continued,
    ];
    let cheerful = samples("cheerful");
    for tone in TONES {
        let lines = samples(tone);
        for kind in kinds {
            assert!(lines.iter().any(|(k, _)| *k == kind), "{tone} {kind:?}");
        }
        for (index, (kind, text)) in lines.iter().enumerate() {
            assert!(!text.trim().is_empty(), "{tone} {kind:?}");
            if tone != "cheerful" {
                assert_ne!(text, &cheerful[index].1, "{tone} {kind:?}");
            }
        }
    }
}
#[test]
fn mood_follows_the_loops() {
    let (mut s, p) = snapshot();
    let shown = |s: &Snapshot| crate::attention::shown(s).mood;
    // An open PR with nothing to do: waiting on the author.
    assert_eq!(shown(&s), Some(Mood::Happy));
    s.reviews.push(round(&p, 1));
    assert_eq!(shown(&s), Some(Mood::Busy));
    finish(&mut s.reviews[0], 1);
    assert_eq!(shown(&s), Some(Mood::Happy), "waiting on the author");
    finish(&mut s.reviews[0], 0);
    assert_eq!(shown(&s), Some(Mood::Happy), "clean");
    s.reviews[0].status = "failed".into();
    assert_eq!(shown(&s), Some(Mood::Worried));
    s.reviews.clear();
    s.settings.automatic = false;
    assert_eq!(shown(&s), Some(Mood::Sleepy), "autopilot off");
    s.settings.automatic = true;
    s.prs.clear();
    assert_eq!(shown(&s), Some(Mood::Sleepy), "nothing open");
}
#[test]
fn settings_check_the_name_and_choices() {
    let mut b = Buddy::default();
    assert_eq!(
        (b.name.as_str(), b.colour.as_str(), b.hat.as_str()),
        ("Bit", "lime", "none")
    );
    assert!(b.enabled);
    assert!(b.validate().is_ok());
    for (field, value, error) in [
        ("name", "  ", "Give your buddy a name."),
        (
            "name",
            "A name that is far too long",
            "Keep your buddy's name to 24 characters.",
        ),
        (
            "name",
            "Bit\nBot",
            "Your buddy's name can't contain line breaks or tabs.",
        ),
        (
            "colour",
            "plaid",
            "'plaid' isn't one of the buddy's colours.",
        ),
        ("hat", "fez", "'fez' isn't one of the buddy's hats."),
        ("tone", "grumpy", "'grumpy' isn't one of the buddy's tones."),
    ] {
        let mut bad = b.clone();
        match field {
            "name" => bad.name = value.into(),
            "colour" => bad.colour = value.into(),
            "hat" => bad.hat = value.into(),
            _ => bad.tone = value.into(),
        }
        assert_eq!(bad.validate().unwrap_err(), error, "{field}");
    }
    let patch: BuddyPatch =
        serde_json::from_value(json!({"name": "  Orbie ", "hat": "wizard"})).unwrap();
    patch.apply(&mut b);
    assert_eq!((b.name.as_str(), b.hat.as_str()), ("Orbie", "wizard"));
    assert!(serde_json::from_value::<BuddyPatch>(json!({"mood": "happy"})).is_err());
}
#[test]
fn saved_state_from_another_version_falls_back_to_defaults() {
    // Older state has no buddy at all.
    let settings: Settings = serde_json::from_value(json!({"automatic": true})).unwrap();
    assert_eq!(settings.buddy, Buddy::default());
    // Newer state may name a hat or colour this version doesn't know.
    let mut b: Buddy = serde_json::from_value(
        json!({"name": "Sprocket", "colour": "plaid", "hat": "fez", "tone": "grumpy"}),
    )
    .unwrap();
    b.normalize();
    assert_eq!(b.name, "Sprocket");
    assert_eq!(
        (b.colour.as_str(), b.hat.as_str(), b.tone.as_str()),
        ("lime", "none", "cheerful")
    );
    assert!(b.validate().is_ok());
}
#[test]
fn the_app_offers_the_same_choices() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../test-fixtures/buddy-options.json")).unwrap();
    let list = |key: &str| -> Vec<String> {
        fixture[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(list("colours"), COLOURS);
    assert_eq!(list("hats"), HATS);
    assert_eq!(list("tones"), TONES);
}
