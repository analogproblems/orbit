---
name: answer-orbit-reviews
description: Use when a pull request has a review from Orbit (headed "Orbit · Codex review" or similar, with findings like "F2 · P1 · Blocking" and an `<!-- orbit-review:` marker) and you are fixing or declining its findings, or when Orbit's CI comment (headed "Orbit · CI") or an `orbit` issue lists failing checks like "I2". Teaches the reply lines Orbit recognises.
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

Orbit also watches CI. When a pull request's checks fail, it posts one comment headed `## Orbit · CI` and edits that same comment as CI changes. When a default branch's checks fail, it opens an issue labelled `orbit`, such as "CI is failing on main". Each failing check is an item, such as **I1** · `CI / Rust tests` · failing at `9f3c2a1`, and the hidden `<!-- orbit-ops thread=... {json} -->` line at the end holds the same items (`id`, `check`, `failing`, `since`, `at`), and whether the problem is still `open` and whether it `passed`. An item still marked failing on a problem that is no longer open is what Orbit last saw before it stopped watching, not a fresh result.

- Fix the failing checks and push. Orbit reads CI after each sync and updates the comment or issue itself: it says when every check passes, and closes its issue once the branch has stayed green for an hour.
- Orbit only reads CI. It never pushes to your branch.

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
- The latest answer about an item counts, and only while the item is failing: when a check fails again later, it needs a new answer. Orbit links your reply rather than repeating your words.

## Where to post

- Post one conversation comment on the pull request with a line per finding, or reply in each finding's inline thread. The summary of a GitHub review isn't read, so don't answer there.
- Keep it short: Orbit reads the 20 most recent comments since the previous round, up to 4,000 characters each.
- Orbit reads replies only from accounts it trusts: the person running Orbit, the trusted accounts set in Orbit, and the pull request's author when the pull request comes from a branch in the repository itself and its author is an owner, member, collaborator or trusted account. If you post as a bot account, the person running Orbit has to add it under **Settings → Trusted accounts**.
