---
name: answer-orbit-reviews
description: Use when a pull request has a review from Orbit (headed "Orbit · Codex review" or similar, with findings like "F2 · P1 · Blocking" and an `<!-- orbit-review:` marker) and you are fixing or declining its findings. Teaches the reply lines Orbit recognises in its next review round.
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

## Where to post

- Post one conversation comment on the pull request with a line per finding, or reply in each finding's inline thread. The summary of a GitHub review isn't read, so don't answer there.
- Keep it short: Orbit reads the 20 most recent comments since the previous round, up to 4,000 characters each.
- Orbit reads replies only from accounts it trusts: the person running Orbit, the trusted accounts set in Orbit, and the pull request's author when the pull request comes from a branch in the repository itself and its author is an owner, member, collaborator or trusted account. If you post as a bot account, the person running Orbit has to add it under **Settings → Trusted accounts**.
