---
name: answer-orbit-reviews
description: Use when a pull request has a review from Orbit (headed "Orbit · Codex review" or similar, with findings like "F2 · P1 · Blocking" and an `<!-- orbit-review:` marker) and you are fixing or declining its findings, when Orbit's comment on a pull request (headed "Orbit" or "Orbit · CI") or an `orbit` issue lists items like "I2", such as failing checks or a proposal to add CI, or when deciding whether you may merge a pull request Orbit reviews. Teaches the reply lines Orbit recognises and when its comment says a pull request is clear to merge.
---

# Answering Orbit reviews

Orbit reviews a pull request in rounds. Each finding has a short ID, such as F2, that is unique within the pull request and never reused. When new commits arrive, the next round checks every earlier finding again and reads the replies posted since the previous round. Replies in the form below are recognised: the reviewer is told what you said, and the next review shows it as **Author said**. The reviewer still checks the code, so a reply is a claim, not a verdict.

## Read the review

- The review is headed `## Orbit · <reviewer> review` and starts with the verdict and how many findings block merging. Each finding is headed like `**F2 · P1 · Blocking · Bug** Title`, followed by the problem, **Breaks when**, **Evidence**, **Suggested fix** and **Done when**. Findings that fit on the diff are inline comments; the rest are in the review text.
- Near the end of the review text, an HTML comment `<!-- orbit-data {...} -->` holds the same findings as JSON (`"version": 1`). Read it instead of the Markdown when you can see the raw text.
  - `findings[]`: `shortId`, `title`, `priority`, `blocking`, `category`, `locations` (the first is where the inline comment is), `problem`, `failureScenario`, `evidence`, `confirmed`, `suggestedFix`, `doneWhen`, `reviewerNotes`.
  - `earlierFindings[]`: findings from earlier rounds, with `status` (`fixed`, `still_open`, `declined_accepted` or `declined_disputed`), the reviewer's `note` and `authorSaid`.
- Fix every finding marked **Blocking**. Fix or decline the others. Earlier findings that are still open or declined (disputed) are still in play and need an answer too.

## Reply

Push your fixes first. Then answer each finding on its own line:

```text
Fixed F2 in 3f9c2a1
Fixed F1, F3 in 3f9c2a1
Declined F4: the caller in src/api.rs rejects an empty list, so this path can't be reached.
```

- `Fixed F2 in <commit>` names the commit that fixes it: at least 7 characters of the SHA of a commit already pushed to the pull request's branch. Orbit checks where the commit is and tells the reviewer when it isn't part of the pull request.
- `Declined F4: <reason>` keeps the reason on one line. Give a reason the reviewer can check against the code; "won't fix" isn't one.
- Name several findings fixed by the same commit together: `Fixed F1, F3 in <commit>`.
- In a finding's own inline thread you can leave out the ID: `Fixed in <commit>` or `Declined: <reason>`.
- Start the line with `Fixed` or `Declined`. A list marker or bold around the line is fine. Other lines reach the reviewer as ordinary comments and aren't recognised.
- Only say `Fixed` when the whole finding is fixed. If part is left, say what remains in a separate comment and leave the finding unanswered, or decline the rest with a reason.
- If you answer a finding twice, the latest answer counts.

## Orbit's CI comments and issues

Orbit also watches CI. When a pull request's checks fail, it posts one comment headed `## Orbit · CI` and edits that same comment as CI changes. On a pull request Orbit reviews automatically, that comment is headed `## Orbit` instead, starts with whether it is clear to merge (see below), and has the CI part under `### CI`. When a default branch's checks fail, it opens an issue labelled `orbit`, such as "CI is failing on main". Each failing check is an item, such as **I1** · `CI / Rust tests` · failing at `9f3c2a1`, and the hidden `<!-- orbit-ops thread=... {json} -->` line at the end holds the same items (`id`, `check`, `failing`, `since`, `at`), and whether the problem is still `open` and whether it `passed`. An item still marked failing on a problem that is no longer open is what Orbit last saw before it stopped watching, not a fresh result.

- Fix the failing checks and push. Orbit reads CI after each sync and updates the comment or issue itself: it says when every check passes, and closes its issue once the branch has stayed green for an hour.
- A job that looks like bad luck rather than the code, such as a lost runner or a network error, Orbit may rerun: the item then says "Orbit reran this job, as it looks like bad luck: …". Wait for that rerun before chasing the failure. "It failed again after it was rerun, so it's likely real." means it is worth fixing. The item in the hidden line then has `reruns`, how many times Orbit reran it on this commit.
- A default branch's issue may say what broke it, such as "It started failing with #42, merged as `7d41e09`; `3b8e6f4` before it passed." Start with that pull request's changes.
- A pull request whose checks passed before its base branch moved on gets an item too, such as **I3** · `main` moved on since the checks at `5e6f7a8` began. Its checks never ran with the base's new changes, so merge the base branch in, push, and let CI run again. The item in the hidden line has `kind` `base_moved` and `base`, the branch, instead of `check`. Orbit marks it done once the checks pass having begun after the base's latest commit.
- A repository where no CI runs on pull requests may get an `orbit` issue titled "Add CI that runs on pull requests", with one item, **I1** · add a workflow that runs on pull requests, and usually a workflow to start from, built from files such as `package.json` or `Cargo.toml`. Check its commands are the ones the project uses, add it as `.github/workflows/ci.yml` (or your own workflow that runs on `pull_request`), and merge it into the default branch. The thread in the hidden line has `kind` `no_ci`, and its item `kind` `no_ci` and `branch` instead of `check`. Orbit closes the issue once a workflow on the default branch runs on pull requests; it doesn't check the commit a `Fixed` line names.
- Orbit reads CI and reruns jobs, but it never pushes to your branch.

Answer each item on its own line, in a reply on the same pull request (a conversation comment) or the same issue:

```text
Taking I2
Fixed I1 in 3f9c2a1
Declined I3: the runner image is out of disk space, which this branch can't change.
Need a person on I4: the deploy key expired, and only an owner can renew it.
```

- `Taking I2` says you are working on it.
- `Fixed I1 in <commit>` names the commit that fixes it: at least 7 characters of the SHA of a commit already pushed to the branch. Orbit checks the item's check on that commit and says on its comment or issue whether it passes there, isn't on the branch, or can't be found. The problem only counts as fixed once the check passes on the branch's latest commit. `Fixed I1` without a commit is read too, and Orbit waits to see the check pass.
- `Declined I3: <reason>` is for a failure that isn't the branch's to fix, such as a broken runner. The person running Orbit is told.
- `Need a person on I4: <question>` is for something only a person can do, such as renewing a secret or changing a setting. It goes on their **Needs you** list.
- Name several items together: `Fixed I1, I2 in <commit>`.
- The latest answer about an item counts, and only while the item is failing. A reply counts only if you post it after Orbit's comment or issue shows the item failing: when a check fails again later, wait for Orbit to say so, then post a new answer. Replies posted earlier don't count, even if you edit them, so post a new reply rather than editing an old one. Orbit links your reply rather than repeating your words.

## When to merge

On a pull request Orbit reviews automatically, Orbit's comment starts with whether it is clear to merge, once it is or once it waits on someone:

- **Clear to merge** at `abc1234` against `main` at `def5678`.
- **Not clear to merge yet.** It conflicts with main: merge main in and fix the conflicts.

Under that line, a hidden `<!-- orbit-ready {json} -->` line says the same for the raw text (`"version": 1`): `clear`, `head` (the commit it was worked out for), `base` (the base branch) and `baseSha` (the base branch's commit it is clear against, empty while it isn't), and `turn`, whose move it is while it isn't clear: `author` (yours), `person` (the person running Orbit, or whoever a rule on GitHub waits for) or `working` (Orbit, CI or GitHub is still at it). `turn` is null when it is clear, or once Orbit stopped working it out, such as while the pull request is a draft.

- Merge only when `clear` is true, `head` is your branch's latest commit and `baseSha` is the base branch's latest commit. An answer about an older commit is stale: wait for Orbit to edit it.
- Clear means Orbit's latest review of that commit is clean, every check on it passed having begun after the base branch's latest commit, and GitHub says it can merge.
- While it isn't clear, the line says the first thing in the way. When `turn` is `author`, it is yours: fix what it names, such as findings, a failing check, or the base branch moving on, then push. Otherwise wait, or ask the person running Orbit.
- Orbit edits the comment only when the answer changes: it turns clear or not, the head or base moves, or it becomes someone else's move. It doesn't edit it for each step Orbit, CI and GitHub take in between, so each edit is worth reading.
- Orbit never merges, approves or requests changes itself.

## Where to post

- Post one conversation comment on the pull request with a line per finding, or reply in each finding's inline thread. The summary of a GitHub review isn't read, so don't answer there.
- Keep it short: Orbit reads the 20 most recent comments since the previous round, up to 4,000 characters each.
- Orbit reads replies only from accounts it trusts: the person running Orbit, the trusted accounts set in Orbit, and the pull request's author when the pull request comes from a branch in the repository itself and its author is an owner, member, collaborator or trusted account. If you post as a bot account, the person running Orbit has to add it under **Settings → Trusted accounts**.
