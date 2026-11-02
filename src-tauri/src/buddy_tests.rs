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
        dev_container: Default::default(),
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
    // The note names its item, so the app can offer the item's fix.
    assert_eq!(note.key.as_deref(), Some("failed:run-1"));
}
#[test]
fn an_item_bit_fixes_on_its_own_is_noted_as_handled() {
    let (mut s, p) = seeded();
    s.settings.buddy.rules.review_again = "do_it".into();
    s.reviews.push(round(&p, 1));
    step(&mut s);
    s.reviews[0].status = "failed".into();
    s.reviews[0].error = Some("Codex exited.".into());
    let item = crate::attention::items(&s).remove(0);
    assert_eq!(will_fix(&s, &item), Some(Fix::ReviewAgain));
    assert_eq!(
        step(&mut s)[1],
        (
            NoteKind::Fixing,
            "b#1: Codex exited. I'm on it!".to_string()
        )
    );
    // No button while Bit is on it.
    assert_eq!(s.buddy_feed.notes.last().unwrap().key, None);

    fixed(&mut s, &item, Fix::ReviewAgain);
    not_fixed(&mut s, &item, Fix::ReviewAgain, "Codex isn't connected.");
    let last: Vec<_> = s.buddy_feed.notes.iter().rev().take(2).collect();
    assert_eq!(
        (last[1].kind, last[1].text.as_str(), last[1].key.as_deref()),
        (
            NoteKind::Fixed,
            "Queued a new review of b#1. Fingers crossed!",
            None
        )
    );
    assert_eq!(
        (last[0].kind, last[0].text.as_str(), last[0].key.as_deref()),
        (
            NoteKind::NeedsYou,
            "I tried to review b#1 again, but it didn't work: Codex isn't connected.",
            Some("failed:run-1")
        )
    );

    // Leave it to me, Ask first, or Bit off: Bit doesn't do it.
    for (rule, enabled) in [("leave_it", true), ("ask_first", true), ("do_it", false)] {
        s.settings.buddy.rules.review_again = rule.into();
        s.settings.buddy.enabled = enabled;
        assert_eq!(will_fix(&s, &item), None, "{rule} {enabled}");
    }
}
#[test]
fn bit_never_does_a_settings_change_on_its_own() {
    let (mut s, _) = seeded();
    s.state_write_error = Some("disk full".into());
    s.settings.automatic = false;
    let item = crate::attention::items(&s).remove(0);
    assert_eq!(item.fix, Some(Fix::TurnOnAutopilot));
    for rule in RULES {
        s.settings.buddy.rules = Rules {
            continue_loop: rule.into(),
            review_again: rule.into(),
            publish: rule.into(),
            sync_now: rule.into(),
            comment_on_prs: rule.into(),
            open_issues: rule.into(),
            rerun_flaky: rule.into(),
            rerun_again: rule.into(),
            propose_ci: rule.into(),
        };
        assert_eq!(will_fix(&s, &item), None, "{rule}");
    }
}
#[test]
fn a_follow_up_chat_on_a_finished_round_repeats_nothing() {
    // A follow-up sets the round running until it ends, then restores the
    // round's outcome, whether the chat answered or failed. Neither the
    // round's result nor its Needs you item is told again.
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
            assert_eq!(step(&mut s), [], "{status} {blocking}");
            assert!(s.reviews[0].end_follow_up(ending.clone()));
            assert_eq!(step(&mut s), [], "{status} {blocking} {ending:?}");
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
        "Heads up! a/b can't sync. timeout. Orbit checks again every 2 minutes."
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
    // A question still being answered stays however many notes follow it.
    let question = asked(&mut s, "How's it going?");
    for _ in 0..MAX_NOTES {
        s.loops[0].stopped = !s.loops[0].stopped;
        step(&mut s);
    }
    assert_eq!(s.buddy_feed.notes.len(), MAX_NOTES);
    assert_eq!(s.buddy_feed.notes[0].id, question);
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
        NoteKind::Fixing,
        NoteKind::Fixed,
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
    // The morning brief is there to read, so it worries nobody.
    s.reviews[0].status = "posted".into();
    s.brief.latest = Some(crate::brief::Brief {
        open: true,
        ..Default::default()
    });
    assert_eq!(crate::attention::items(&s)[0].kind, AttentionKind::Brief);
    assert_eq!(shown(&s), Some(Mood::Happy));
    s.reviews[0].status = "failed".into();
    assert_eq!(shown(&s), Some(Mood::Worried));
    s.brief = Default::default();
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
        (
            b.name.as_str(),
            b.colour.as_str(),
            b.hat.as_str(),
            b.look.as_str()
        ),
        ("Bit", "lime", "none", "glim")
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
        ("look", "blob", "'blob' isn't one of the buddy's looks."),
        ("tone", "grumpy", "'grumpy' isn't one of the buddy's tones."),
        (
            "answerer",
            "eliza",
            "'eliza' can't answer the buddy's questions.",
        ),
        (
            "ollama_url",
            "localhost:11434",
            "Ollama's address starts with http:// or https://, such as http://localhost:11434.",
        ),
        (
            "brief_at",
            "9am",
            "'9am' isn't a time for the morning brief. Give hours and minutes, such as 09:00.",
        ),
        (
            "time_zone",
            "Mars/Olympus",
            "'Mars/Olympus' isn't a time zone Orbit knows. Choose one from the list, such as Europe/London.",
        ),
    ] {
        let mut bad = b.clone();
        match field {
            "name" => bad.name = value.into(),
            "colour" => bad.colour = value.into(),
            "hat" => bad.hat = value.into(),
            "look" => bad.look = value.into(),
            "answerer" => bad.answerer = value.into(),
            "ollama_url" => bad.ollama_url = value.into(),
            "brief_at" => bad.brief_at = value.into(),
            "time_zone" => bad.time_zone = value.into(),
            _ => bad.tone = value.into(),
        }
        assert_eq!(bad.validate().unwrap_err(), error, "{field}");
    }
    let patch: BuddyPatch =
        serde_json::from_value(json!({"name": "  Orbie ", "hat": "wizard", "look": "fizz"}))
            .unwrap();
    patch.apply(&mut b);
    assert_eq!(
        (b.name.as_str(), b.hat.as_str(), b.look.as_str()),
        ("Orbie", "wizard", "fizz")
    );
    assert!(serde_json::from_value::<BuddyPatch>(json!({"mood": "happy"})).is_err());
    // The morning brief comes at 9 until you choose.
    assert!(b.brief);
    assert_eq!(b.brief_at, "09:00");
    let patch: BuddyPatch =
        serde_json::from_value(json!({"brief": false, "briefAt": " 17:30 "})).unwrap();
    patch.apply(&mut b);
    assert_eq!((b.brief, b.brief_at.as_str()), (false, "17:30"));
    assert!(b.validate().is_ok());
    // By this computer's clock until you choose a time zone.
    assert_eq!(b.time_zone, "");
    let patch: BuddyPatch =
        serde_json::from_value(json!({"timeZone": " America/New_York "})).unwrap();
    patch.apply(&mut b);
    assert_eq!(b.time_zone, "America/New_York");
    assert!(b.validate().is_ok());
    // Codex answers until you choose another.
    assert_eq!(b.answered_by(), Answerer::Codex);
    let patch: BuddyPatch = serde_json::from_value(json!({"answerer": "claude"})).unwrap();
    patch.apply(&mut b);
    assert_eq!(b.answered_by(), Answerer::Claude);
    b.answerer = "antigravity".into();
    assert_eq!(b.answered_by(), Answerer::Antigravity);
    b.answerer = "ollama".into();
    assert_eq!(b.answered_by(), Answerer::Ollama);
    assert_eq!(b.answered_by().provider(), None);

    // Every rule starts on Ask first; a patch changes only the rules it names.
    assert!(RULES
        .iter()
        .all(|r| ["do_it", "ask_first", "leave_it"].contains(r)));
    assert_eq!(b.rules, Rules::default());
    assert_eq!(b.rules.of(Fix::Publish), "ask_first");
    let patch: BuddyPatch = serde_json::from_value(json!({"rules": {"publish": "do_it"}})).unwrap();
    patch.apply(&mut b);
    assert_eq!(b.rules.of(Fix::Publish), "do_it");
    assert_eq!(b.rules.of(Fix::ContinueLoop), "ask_first");
    assert!(b.validate().is_ok());
    let mut bad = b.clone();
    bad.rules.sync_now = "always".into();
    assert_eq!(
        bad.validate().unwrap_err(),
        "'always' isn't one of the buddy's rules."
    );
    assert!(serde_json::from_value::<BuddyPatch>(json!({"rules": {"approve": "do_it"}})).is_err());
}
#[test]
fn saved_state_from_another_version_falls_back_to_defaults() {
    // Older state has no buddy at all.
    let settings: Settings = serde_json::from_value(json!({"automatic": true})).unwrap();
    assert_eq!(settings.buddy, Buddy::default());
    // Older state has no look, so Bit takes the default one and keeps its
    // colour and hat.
    let b: Buddy = serde_json::from_value(json!({"colour": "pink", "hat": "crown"})).unwrap();
    assert_eq!(
        (b.colour.as_str(), b.hat.as_str(), b.look.as_str()),
        ("pink", "crown", "glim")
    );
    // Newer state may name a hat, colour or look this version doesn't know.
    let mut b: Buddy = serde_json::from_value(
        json!({"name": "Sprocket", "colour": "plaid", "hat": "fez", "look": "blob", "tone": "grumpy", "answerer": "eliza"}),
    )
    .unwrap();
    b.normalize();
    assert_eq!(b.answerer, "codex");
    assert_eq!(b.name, "Sprocket");
    assert_eq!(
        (
            b.colour.as_str(),
            b.hat.as_str(),
            b.look.as_str(),
            b.tone.as_str()
        ),
        ("lime", "none", "glim", "cheerful")
    );
    assert!(b.validate().is_ok());
    // A brief time Orbit can't use, and none from before briefs.
    let mut b: Buddy = serde_json::from_value(json!({"briefAt": "25:00", "brief": false})).unwrap();
    b.normalize();
    assert_eq!((b.brief, b.brief_at.as_str()), (false, "09:00"));
    let b: Buddy = serde_json::from_value(json!({"name": "Sprocket"})).unwrap();
    assert_eq!((b.brief, b.brief_at.as_str()), (true, "09:00"));
    // A time zone this version doesn't know, and none from before zones:
    // the computer's clock.
    let mut b: Buddy = serde_json::from_value(json!({"timeZone": "Mars/Olympus"})).unwrap();
    b.normalize();
    assert_eq!(b.time_zone, "");
    let mut b: Buddy = serde_json::from_value(json!({"timeZone": "Asia/Tokyo"})).unwrap();
    b.normalize();
    assert_eq!(b.time_zone, "Asia/Tokyo");
    // An Ollama address Orbit can't use.
    let mut b: Buddy =
        serde_json::from_value(json!({"ollamaUrl": "file:///etc", "ollamaModel": "a\nb"})).unwrap();
    b.normalize();
    assert_eq!(
        (b.ollama_url.as_str(), b.ollama_model.as_str()),
        (crate::ollama::DEFAULT_URL, "")
    );
    // Older state has no rules; newer state may name a rule or a fix this
    // version doesn't know.
    let mut b: Buddy = serde_json::from_value(
        json!({"rules": {"publish": "do_it", "sync_now": "sometimes", "approve": "do_it"}}),
    )
    .unwrap();
    b.normalize();
    assert_eq!(
        b.rules,
        Rules {
            publish: "do_it".into(),
            ..Rules::default()
        }
    );
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
    assert_eq!(list("looks"), LOOKS);
    assert_eq!(list("tones"), TONES);
    assert_eq!(list("answerers"), ANSWERERS);
    assert_eq!(list("rules"), RULES);
    let names = |values: Vec<serde_json::Value>| -> Vec<String> {
        values
            .into_iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    let fixes = FIXES.map(|f| serde_json::to_value(f).unwrap());
    assert_eq!(list("fixes"), names(fixes.to_vec()));
    let questions = [
        Question::NeedsYou,
        Question::Running,
        Question::Loops,
        Question::Repositories,
        Question::Setup,
    ];
    let asked = questions.map(|q| serde_json::to_value(q).unwrap());
    assert_eq!(list("questions"), names(asked.to_vec()));
    assert_eq!(
        fixture["memoryLimits"],
        json!({"general": MEMORY_GENERAL, "perRepository": MEMORY_PER_REPO})
    );
}

#[test]
fn settings_connections_and_posting_again_always_ask_first() {
    let mut rules = Rules::default();
    for rule in rules.each_mut() {
        *rule = "do_it".into();
    }
    for fix in FIXES {
        let asks = matches!(
            fix,
            Fix::TurnOnAutopilot | Fix::CheckConnections | Fix::PostHereAgain
        );
        assert_eq!(rules.of(fix) == "ask_first", asks, "{fix:?}");
    }
}
#[test]
fn rules_use_the_names_the_app_sends() {
    // The app keys rules by fix name, both for what it reads and for the one
    // rule at a time it saves.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../test-fixtures/buddy-options.json")).unwrap();
    let defaults = &fixture["defaultRules"];
    assert_eq!(serde_json::to_value(Rules::default()).unwrap(), *defaults);
    let ruled = FIXES.into_iter().filter(|&f| {
        !matches!(
            f,
            Fix::TurnOnAutopilot | Fix::CheckConnections | Fix::PostHereAgain
        )
    });
    for fix in ruled {
        let name = serde_json::to_value(fix).unwrap();
        let name = name.as_str().unwrap();
        assert!(defaults.get(name).is_some(), "{name} has no rule");
        let mut b = Buddy::default();
        let patch: BuddyPatch = serde_json::from_value(json!({"rules": {name: "do_it"}})).unwrap();
        patch.apply(&mut b);
        assert_eq!(b.rules.of(fix), "do_it", "{name}");
        let saved = serde_json::to_value(&b).unwrap();
        assert_eq!(saved["rules"][name], "do_it", "{name}");
        let loaded: Buddy = serde_json::from_value(saved).unwrap();
        assert_eq!(loaded.rules.of(fix), "do_it", "{name}");
    }
    // So are the rules for what Bit does on GitHub about failing CI.
    let posts = fixture["posts"].as_array().unwrap();
    assert_eq!(posts.len(), 5);
    for name in posts {
        let name = name.as_str().unwrap();
        assert!(defaults.get(name).is_some(), "{name} has no rule");
        let mut b = Buddy::default();
        let patch: BuddyPatch =
            serde_json::from_value(json!({"rules": {name: "leave_it"}})).unwrap();
        patch.apply(&mut b);
        let saved = serde_json::to_value(&b).unwrap();
        assert_eq!(saved["rules"][name], "leave_it", "{name}");
    }
    let b = Buddy::default();
    assert_eq!(
        (
            b.rules.comment_on_prs.as_str(),
            b.rules.open_issues.as_str(),
            b.rules.rerun_flaky.as_str(),
            b.rules.rerun_again.as_str(),
            b.rules.propose_ci.as_str()
        ),
        ("do_it", "do_it", "do_it", "ask_first", "ask_first")
    );
}

/// A clean loop says whether it is clear to merge, and why not.
#[test]
fn a_clean_loop_says_whether_it_may_merge() {
    let (mut s, p) = seeded();
    let mut r = round(&p, 1);
    finish(&mut r, 0);
    s.reviews.push(r);
    s.settings.watch_ci = true;
    // Only for a pull request Orbit reviews automatically.
    assert_eq!(
        ask_and_read(&mut s, Question::Loops).1,
        ["b#1: round 1 of 4, clean."]
    );
    s.prs[0].head_repo = "a/b".into();
    s.prs[0].author_association = "OWNER".into();
    assert_eq!(
        ask_and_read(&mut s, Question::Loops).1,
        ["b#1: round 1 of 4, clean. Not clear to merge yet. Orbit hasn't read the checks on aaaaaaa yet."]
    );
    // A loop that isn't clean already says what it waits for.
    let mut r = round(&p, 2);
    finish(&mut r, 1);
    s.reviews.push(r);
    let lines = ask_and_read(&mut s, Question::Loops).1;
    assert!(
        lines[0].starts_with("b#1: round 2 of 4, waiting for the author."),
        "{lines:?}"
    );
    assert!(!lines[0].contains("merge"), "{lines:?}");
}
fn ask_and_read(
    s: &mut Snapshot,
    question: Question,
) -> (String, Vec<String>, Vec<Option<String>>) {
    ask(s, question, &crate::ask::Lookups::default());
    let note = s.buddy_feed.notes.last().unwrap();
    assert_eq!(note.kind, NoteKind::Answer);
    assert_eq!(note.question.as_deref(), Some(question.text()));
    (
        note.text.clone(),
        note.lines.iter().map(|l| l.text.clone()).collect(),
        note.lines.iter().map(|l| l.key.clone()).collect(),
    )
}
#[test]
fn quick_answers_come_from_orbits_state() {
    let (mut s, p) = seeded();
    assert_eq!(
        ask_and_read(&mut s, Question::NeedsYou),
        (
            "Nothing needs you right now. 🎉".to_string(),
            vec![],
            vec![]
        )
    );
    assert_eq!(
        ask_and_read(&mut s, Question::Running).0,
        "Nothing is running. All quiet!"
    );
    assert_eq!(
        ask_and_read(&mut s, Question::Loops).0,
        "No loops are open right now."
    );

    // A loop at its round limit, a round running on another PR, a sync.
    s.settings.repositories[0].round_limit = 1;
    let mut r = round(&p, 1);
    finish(&mut r, 2);
    s.reviews.push(r);
    let mut other = p.clone();
    other.id = "a/b#2".into();
    other.number = 2;
    s.prs.push(other.clone());
    s.loops.push(LoopControl {
        pr_id: other.id.clone(),
        stopped: false,
        extra_rounds: 1,
    });
    let mut running = round(&other, 2);
    running.status = "queued".into();
    s.reviews.push(running);
    s.polling = true;
    s.sync_failures.push(SyncFailure {
        repo: "a/b".into(),
        count: 1,
        error: "timeout".into(),
    });

    let limit = Some("loop:a/b#1:Round limit reached.".to_string());
    assert_eq!(
        ask_and_read(&mut s, Question::NeedsYou),
        (
            "1 thing needs you. Here's the list:".to_string(),
            vec!["b#1: Round limit reached.".to_string()],
            vec![limit.clone()]
        )
    );
    assert_eq!(
        ask_and_read(&mut s, Question::Running),
        (
            "2 things are running. Here's what:".to_string(),
            vec![
                "Syncing with GitHub.".to_string(),
                "b#2: round 1 is queued.".to_string()
            ],
            vec![None, None]
        )
    );
    assert_eq!(
        ask_and_read(&mut s, Question::Loops),
        (
            "2 loops are open: 1 needs you, 1 reviewing. Here's each one:".to_string(),
            vec![
                "b#1: round 1 of 1, needs you. Round limit reached.".to_string(),
                "b#2: round 1 of 2, reviewing.".to_string()
            ],
            vec![limit, None]
        )
    );
    assert_eq!(
        ask_and_read(&mut s, Question::Repositories),
        (
            "You have 1 repository. Here's how they're doing:".to_string(),
            vec![
                "a/b: 2 open pull requests, autopilot on. The last sync failed: timeout"
                    .to_string()
            ],
            vec![None]
        )
    );

    // Calm and dry say it their way; autopilot being off is mentioned.
    s.settings.buddy.tone = "calm".into();
    s.settings.automatic = false;
    assert_eq!(
        ask_and_read(&mut s, Question::NeedsYou).0,
        "1 thing needs you:"
    );
    assert_eq!(
        ask_and_read(&mut s, Question::Repositories).0,
        "Autopilot is off, so nothing is reviewed automatically. 1 repository:"
    );
    s.settings.buddy.tone = "dry".into();
    s.settings.repositories.clear();
    assert_eq!(
        ask_and_read(&mut s, Question::Repositories).0,
        "Autopilot is off, so nothing is reviewed automatically. No repositories. Hard to review nothing."
    );
}
#[test]
fn every_tone_answers_every_question_its_own_way() {
    for n in [0, 1, 3] {
        for tone in ["calm", "dry"] {
            for (cheerful, other) in [
                (
                    say::needs_you_answer("cheerful", n),
                    say::needs_you_answer(tone, n),
                ),
                (
                    say::running_answer("cheerful", n),
                    say::running_answer(tone, n),
                ),
                (
                    say::repositories_answer("cheerful", n),
                    say::repositories_answer(tone, n),
                ),
            ] {
                assert_ne!(cheerful, other, "{tone} {n}");
            }
        }
    }
    for summary in [None, Some("1 loop is open: 1 clean.")] {
        assert_ne!(
            say::loops_answer("cheerful", summary),
            say::loops_answer("dry", summary)
        );
    }
}
#[test]
fn a_question_orbit_closed_on_says_so() {
    let (mut s, _) = snapshot();
    let id = asked(&mut s, "Anything new?");
    assert!(s.buddy_feed.notes[0].pending);
    recover(&mut s.buddy_feed);
    let text = "Orbit closed before I could answer. Ask me again?";
    assert!(!s.buddy_feed.notes[0].pending);
    assert_eq!(s.buddy_feed.notes[0].text, text);
    // An answer that arrives late doesn't replace it.
    answered(&mut s, id, "Late.".into(), vec![], vec![]);
    assert_eq!(s.buddy_feed.notes[0].text, text);
    // Each tone says it couldn't answer in its own way.
    let said: std::collections::HashSet<String> = TONES
        .iter()
        .map(|tone| say::no_answer(tone, "Codex disconnected."))
        .collect();
    assert_eq!(said.len(), TONES.len());
    assert!(said.iter().all(|t| t.contains("Codex disconnected.")));
}
#[test]
fn memory_keeps_to_its_limits_and_makes_room_by_forgetting_first() {
    let (mut s, _) = snapshot();
    let general = |line: &str, kind| MemoryChange {
        repo: None,
        line: line.into(),
        kind,
    };
    let about = |repo: &str, line: &str, kind| MemoryChange {
        repo: Some(repo.into()),
        line: line.into(),
        kind,
    };
    s.buddy_feed.memory.general = (0..MEMORY_GENERAL).map(|i| format!("Line {i}.")).collect();
    s.buddy_feed.memory.repositories.insert(
        "a/b".into(),
        (0..MEMORY_PER_REPO).map(|i| format!("Fact {i}.")).collect(),
    );
    let id = asked(&mut s, "Remember more?");
    answered(
        &mut s,
        id,
        "Done.".into(),
        vec![],
        vec![
            general("New line.", MemoryKind::Remember),
            about("A/B", "Another fact.", MemoryKind::Remember),
            // Already kept, in any case: not added twice.
            general("line 3.", MemoryKind::Remember),
            // Forgotten first, whatever the order, which makes room.
            general("LINE 0.", MemoryKind::Forget),
            general("Not kept.", MemoryKind::Forget),
        ],
    );
    let memory = &s.buddy_feed.memory;
    assert_eq!(memory.general.len(), MEMORY_GENERAL);
    assert_eq!(memory.general[0], "Line 1.");
    assert_eq!(memory.general.last().unwrap(), "New line.");
    assert_eq!(memory.repositories["a/b"].len(), MEMORY_PER_REPO);
    assert!(!memory.repositories["a/b"].contains(&"Another fact.".into()));
    assert_eq!(
        s.buddy_feed.notes.last().unwrap().memory,
        [
            general("Line 0.", MemoryKind::Forget),
            general("New line.", MemoryKind::Remember),
            about("A/B", "Another fact.", MemoryKind::Full),
        ]
    );

    // A repository's last line forgotten leaves no empty entry, and you
    // can forget a line yourself.
    assert!(forget(&mut s, Some("A/B"), "fact 0."));
    assert!(!forget(&mut s, Some("A/B"), "fact 0."));
    for i in 1..MEMORY_PER_REPO {
        assert!(forget(&mut s, Some("a/b"), &format!("Fact {i}.")));
    }
    assert!(s.buddy_feed.memory.repositories.is_empty());
    assert!(forget(&mut s, None, "New line."));

    // An answer that arrives after its question ended changes nothing.
    let before = s.buddy_feed.memory.clone();
    answered(
        &mut s,
        id,
        "Late.".into(),
        vec![],
        vec![general("Late line.", MemoryKind::Remember)],
    );
    assert_eq!(s.buddy_feed.memory, before);
}
#[test]
fn state_saved_before_memory_loads_with_none() {
    let feed: Feed = serde_json::from_value(json!({"notes": [], "nextId": 3})).unwrap();
    assert_eq!(feed.memory, Memory::default());
    let saved = serde_json::to_value(Feed::default()).unwrap();
    assert_eq!(saved["memory"], json!({"general": [], "repositories": {}}));
}

fn story(number: u64, updated_at: &str) -> crate::ledger::History {
    use crate::ledger::{
        writeup::{Decision, DecisionKind, MadeBy, WriteUp},
        Flag, FlagKind, Glance, History,
    };
    let decision = |statement: &str, attention| Decision {
        kind: DecisionKind::Pushback,
        statement: statement.into(),
        detail: None,
        made_by: MadeBy::Both,
        area: "Webhooks".into(),
        quote: None,
        url: String::new(),
        attention,
        attention_reason: Some("Decide whether that is right.".into()),
        supersedes: None,
    };
    History {
        repo: "A/b".into(),
        number,
        updated_at: updated_at.into(),
        flags: vec![
            Flag {
                kind: FlagKind::MergedWithOpenFindings,
                attention: Glance::Act,
                text: "Merged while F1 (Token logged) was still open.".into(),
                finding_id: Some("f1".into()),
                round: None,
                url: None,
            },
            Flag {
                kind: FlagKind::NoReviewBeforeMerge,
                attention: Glance::Look,
                text: "Merged before any reviewer round Orbit can see.".into(),
                finding_id: None,
                round: None,
                url: None,
            },
        ],
        writeup: Some(WriteUp {
            version: 1,
            repo: "A/b".into(),
            number,
            input_hash: String::new(),
            writer: "Codex".into(),
            model: "m".into(),
            written_at: String::new(),
            summary: String::new(),
            areas: vec![],
            decisions: vec![
                decision(
                    "The old secret stays valid if the job is off.",
                    Some(Glance::Act),
                ),
                decision("Only good to know.", Some(Glance::Look)),
            ],
            finding_notes: vec![],
        }),
        ..Default::default()
    }
}
fn decide_notes(s: &Snapshot) -> Vec<(Option<String>, String)> {
    s.buddy_feed
        .notes
        .iter()
        .filter(|n| n.kind == NoteKind::Decide)
        .map(|n| (n.pr_id.clone(), n.text.clone()))
        .collect()
}

#[test]
fn bit_notes_each_new_thing_to_decide_once_but_not_history() {
    let (mut s, _) = snapshot();
    let old = story(1, "2020-01-01T00:00:00Z");
    // The first look only records what is already there.
    assert!(observe_decisions(
        &mut s,
        &to_decide(std::slice::from_ref(&old))
    ));
    assert!(decide_notes(&s).is_empty());
    assert_eq!(s.buddy_feed.decisions_seen.len(), 2);
    assert!(s.buddy_feed.decisions_since.contains_key("a/b"));
    assert!(!observe_decisions(
        &mut s,
        &to_decide(std::slice::from_ref(&old))
    ));

    // A pull request that changed since gets one note for each thing to
    // decide, and only once.
    let new = story(2, "2999-01-01T00:00:00Z");
    let items = to_decide(&[new.clone(), old.clone()]);
    assert!(observe_decisions(&mut s, &items));
    assert_eq!(
        decide_notes(&s),
        [
            (
                Some("a/b#2".into()),
                "Something to decide on b#2: Merged while F1 (Token logged) was still open.".into()
            ),
            (
                Some("a/b#2".into()),
                "Something to decide on b#2: The old secret stays valid if the job is off. Decide whether that is right.".into()
            ),
        ]
    );
    assert!(!observe_decisions(&mut s, &items));
    assert_eq!(decide_notes(&s).len(), 2);

    // A disconnected repository is forgotten; with Bit off, nothing is
    // written.
    s.settings.repositories.clear();
    assert!(observe_decisions(&mut s, &[]));
    assert!(s.buddy_feed.decisions_since.is_empty());
    assert!(s.buddy_feed.decisions_seen.is_empty());
    let (mut quiet, _) = snapshot();
    quiet.settings.buddy.enabled = false;
    observe_decisions(&mut quiet, &[]);
    observe_decisions(&mut quiet, &to_decide(&[new]));
    assert!(decide_notes(&quiet).is_empty());
    assert_eq!(quiet.buddy_feed.decisions_seen.len(), 2);
}

#[test]
fn bit_never_notes_a_decision_twice_however_many_there_are() {
    let (mut s, _) = snapshot();
    observe_decisions(&mut s, &[]);
    // Far more than a fixed list of seen keys would hold.
    let prs: Vec<crate::ledger::History> = (1..=700)
        .map(|n| story(n, "2999-01-01T00:00:00Z"))
        .collect();
    let items = to_decide(&prs);
    assert_eq!(items.len(), 1400);
    let written = |s: &Snapshot| s.buddy_feed.next_id;
    let start = written(&s);
    observe_decisions(&mut s, &items);
    let first = written(&s);
    assert_eq!(first - start, 1400);
    // The same history again, after a restart, and with Bit hidden then
    // shown again: no more notes.
    assert!(!observe_decisions(&mut s, &items));
    s.buddy_feed = serde_json::from_value(serde_json::to_value(&s.buddy_feed).unwrap()).unwrap();
    assert!(!observe_decisions(&mut s, &items));
    s.settings.buddy.enabled = false;
    observe_decisions(&mut s, &items);
    s.settings.buddy.enabled = true;
    observe_decisions(&mut s, &items);
    assert_eq!(written(&s), first);

    // A key still in the history is kept however old; one gone from it
    // for a month is forgotten.
    let long_ago = "2020-01-01T00:00:00.000Z".to_string();
    for seen in s.buddy_feed.decisions_seen.values_mut() {
        *seen = long_ago.clone();
    }
    assert!(observe_decisions(&mut s, &items[2..]));
    assert_eq!(s.buddy_feed.decisions_seen.len(), 1398);
    assert!(s
        .buddy_feed
        .decisions_seen
        .values()
        .all(|seen| *seen > long_ago));
    assert_eq!(written(&s), first);
}
#[test]
fn bit_continues_a_loop_on_its_own_only_once() {
    let (mut s, p) = seeded();
    s.settings.buddy.rules.continue_loop = "do_it".into();
    let item = Attention {
        key: format!("loop:{}:Round limit reached.", p.id),
        kind: AttentionKind::Loop,
        pr_id: Some(p.id.clone()),
        review_id: None,
        approval_id: None,
        repo: Some(p.repo.clone()),
        reason: "Round limit reached.".into(),
        fix: Some(Fix::ContinueLoop),
    };
    assert_eq!(will_fix(&s, &item), Some(Fix::ContinueLoop));
    s.loops.push(LoopControl {
        pr_id: p.id.clone(),
        stopped: false,
        extra_rounds: CONTINUE_ROUNDS,
    });
    assert_eq!(will_fix(&s, &item), None);
}
#[test]
fn runs_waiting_for_a_runner_are_one_heads_up_linked_from_how_are_my_repositories() {
    let (mut s, _) = seeded();
    let run = crate::runners::Waiting {
        id: 1,
        name: "Release".into(),
        ..Default::default()
    };
    crate::problems::see_waiting(&mut s.problems, "a/b", &[run], true, chrono::Utc::now());
    assert_eq!(
        step(&mut s),
        [(
            NoteKind::NeedsYou,
            "Heads up! A run of Release in a/b has waited more than 30 minutes for a runner."
                .to_string()
        )]
    );
    assert!(step(&mut s).is_empty(), "told once");
    let (_, _, keys) = ask_and_read(&mut s, Question::Repositories);
    assert_eq!(keys, [Some(s.problems[0].item_key())]);
}
#[test]
fn a_security_alert_is_one_heads_up_and_counts_in_how_are_my_repositories() {
    use crate::security::tests::{dependabot, secret};
    use crate::security::{alert, Kind, RepoSecurity, Source};
    let (mut s, _) = seeded();
    let kind = |source, alerts: &[serde_json::Value]| Kind {
        source,
        alerts: alerts.iter().filter_map(|v| alert(source, v)).collect(),
        ..Default::default()
    };
    let read = RepoSecurity {
        repo: "a/b".into(),
        read_at: "2026-11-01T09:00:00.000Z".into(),
        kinds: vec![
            kind(
                Source::Dependabot,
                &[
                    dependabot(1, "lodash", "low"),
                    dependabot(2, "qs", "medium"),
                ],
            ),
            kind(Source::SecretScanning, &[secret(3)]),
        ],
        ..Default::default()
    };
    crate::problems::see_security(&mut s.problems, &read, &[], chrono::Utc::now());
    s.security.push(read);
    assert_eq!(
        step(&mut s),
        [(
            NoteKind::NeedsYou,
            "Heads up! GitHub found a secret in a/b: a GitHub Personal Access Token in src/config.ts line 4. Revoke it, then close its alert on GitHub."
                .to_string()
        )]
    );
    assert!(step(&mut s).is_empty(), "told once");
    let (_, lines, keys) = ask_and_read(&mut s, Question::Repositories);
    assert!(
        lines[0].ends_with("autopilot on. 1 security alert needs you, and 2 more are open."),
        "{lines:?}"
    );
    let secrets = s
        .problems
        .iter()
        .find(|p| p.source == Some(Source::SecretScanning))
        .unwrap();
    assert_eq!(keys, [Some(secrets.item_key())]);
    // Once the secret is gone, the rest are only counted.
    s.problems
        .retain(|p| p.source != Some(Source::SecretScanning));
    let (_, lines, keys) = ask_and_read(&mut s, Question::Repositories);
    assert!(
        lines[0].ends_with("autopilot on. 2 security alerts are open, none high or critical."),
        "{lines:?}"
    );
    assert_eq!(keys, [None]);
    // Risky workflow settings follow.
    let text = "on: issues\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo ${{ github.event.issue.title }}\n";
    let path = ".github/workflows/ci.yml".to_string();
    let setup = crate::workflows::Setup {
        repo: "a/b".into(),
        sha: "1".repeat(40),
        checks: crate::workflows::CHECKS,
        workflows: vec![crate::workflows::Workflow {
            risks: crate::risks::check(&path, text, "a").unwrap(),
            path,
            checked: true,
            ..Default::default()
        }],
        ..Default::default()
    };
    crate::problems::see_risks(&mut s.problems, &setup, chrono::Utc::now());
    let (_, lines, _) = ask_and_read(&mut s, Question::Repositories);
    assert!(
        lines[0].ends_with(
            "none high or critical. Its workflows have 2 risky settings, 1 of them high."
        ),
        "{lines:?}"
    );
}
#[test]
fn a_failing_default_branch_is_one_heads_up_and_shows_in_how_are_my_repositories() {
    let (mut s, _) = seeded();
    let ci = crate::ci::RepoCi {
        repo: "a/b".into(),
        branch: "main".into(),
        head: crate::ci::CommitCi {
            sha: "1".repeat(40),
            state: crate::ci::CiState::Failing,
            failing: vec![crate::ci::Check {
                name: "CI / Rust".into(),
                url: String::new(),
            }],
            ..Default::default()
        },
        complete: true,
        ..Default::default()
    };
    crate::problems::observe(&mut s.problems, &ci, &[], chrono::Utc::now());
    s.ci.push(ci);
    assert_eq!(
        step(&mut s),
        [(
            NoteKind::NeedsYou,
            "Heads up! CI is failing on main in a/b since 1111111: CI / Rust.".to_string()
        )]
    );
    assert!(step(&mut s).is_empty(), "told once");
    let (_, lines, keys) = ask_and_read(&mut s, Question::Repositories);
    assert!(
        lines[0].ends_with("autopilot on. CI is failing on main: CI / Rust."),
        "{lines:?}"
    );
    assert_eq!(keys, [Some(s.problems[0].item_key())]);
    // Checks it couldn't all read: neither passing nor failing.
    s.ci[0].head.state = crate::ci::CiState::Running;
    s.ci[0].head.failing.clear();
    s.ci[0].head.incomplete = true;
    let (_, lines, _) = ask_and_read(&mut s, Question::Repositories);
    assert!(
        lines[0].ends_with("Orbit couldn't read every check on main."),
        "{lines:?}"
    );
    // Caches near GitHub's usual limit, after CI.
    s.actions_storage.push(crate::actions_storage::Storage {
        repo: "A/B".into(),
        cache_bytes: crate::actions_storage::NEAR,
        ..Default::default()
    });
    let (_, lines, _) = ask_and_read(&mut s, Question::Repositories);
    assert!(
        lines[0].ends_with("Orbit couldn't read every check on main. Its caches hold 9.0 GB, near GitHub's usual 10 GB limit, so GitHub may soon drop the ones used least."),
        "{lines:?}"
    );
    // With Watch CI off, the answer says nothing about either.
    s.settings.watch_ci = false;
    let (_, lines, _) = ask_and_read(&mut s, Question::Repositories);
    assert!(lines[0].ends_with("autopilot on."), "{lines:?}");
}
