# Orbit architecture

How Orbit is built and why it behaves the way it does. For what each control does, see the [user guide](user-guide.md). The code map is in the [README](../README.md#architecture).

## Overview

Orbit is a Tauri 2 desktop app. The React frontend renders a snapshot of the engine's state and sends a small set of named commands. The Rust engine does everything else: polling GitHub, scheduling reviews, preparing worktrees, talking to Codex, publishing, and saving state. Polling and review orchestration run in Rust, independent of which view is open. Orbit must remain running; this version does not install a background daemon or GitHub webhook.

Orbit uses the `gh`, `git`, and `codex` executables installed on your `PATH` rather than bundling a Codex binary. GitHub CLI and Codex manage their own authentication; Orbit does not request or store copies of your tokens.

## Codex app-server protocol

- Codex uses newline-delimited app-server messages over stdio: `initialize` → client `initialized` → `thread/start` → `review/start`; follow-ups use `thread/resume` and `turn/start`. Streaming and final review items are routed by thread and turn IDs.
- The environment setup command runs in the new worktree through Codex's sandboxed `command/exec`, with a five-minute timeout.
- Approvals are routed to the user explicitly (`approvalsReviewer: "user"`), even when a different reviewer is configured globally in Codex. Command and file-change escalation requests are displayed for a person to approve or deny; unknown interactive requests are not automatically granted.
- The compatibility baseline is Codex CLI **0.156.1** and the [official app-server protocol](https://developers.openai.com/codex/app-server). Re-reviews use Codex's `custom` review target, because review mode at 0.156.1 runs with no thread history and no turn-level developer instructions, so the previous round has to go in the target itself (see `src-tauri/src/engine/rounds.rs`).

### Model pinning

Every new review pins an explicit model and effort when queued. Orbit validates that pair against the connected account's catalog before enqueueing and preparation. Changing workspace Settings affects future runs only. Model selection uses the catalog's `model` identifier, not its display ID. How empty settings are resolved is described in the [user guide](user-guide.md#settings-and-when-changes-take-effect).

Orbit sends the pinned model, matching `review_model`, and reasoning effort on thread creation. It requires Codex's returned model and effort to match exactly and persists the confirmation before `review/start`. Every follow-up resumes with its pinned pair, verifies and saves the confirmation, and passes the same model and effort on `turn/start`. Missing/null or mismatched responses stop execution. Threads created during a stopped or rejected preparation are archived; if archiving fails, Orbit tries to unsubscribe and records the thread ID in the chat. Orbit does not request provider fallback or modify your Codex configuration.

For older chats missing a recorded selection, Orbit unsubscribes its idle thread before the first legacy resume so Codex can apply overrides (0.156.1 applies changed overrides to an idle loaded thread only after its subscribers detach). If another subscriber prevents that change, response verification stops the turn.

## GitHub

- Each sync lists each connected repository's open PRs with one request and fetches an individual PR only when it is new or has changed. A repository that fails to sync keeps its last-known PRs and shows an error while the others continue, and autopilot only acts on repositories that synced successfully in the current session.
- Before publication, Orbit checks that the PR is still open and both SHAs still match. GitHub does not offer an atomic compare-and-comment endpoint, so a commit can still arrive in the small interval between that check and the POST; comments always identify the exact reviewed SHAs.
- A stable marker in each comment and review reconciles ambiguous retries after a timeout or restart. A local instance lock prevents two Orbit processes from using the same state directory concurrently. This is local deduplication, not coordination across multiple computers.

## Isolation and its limits

Orbit fetches GitHub's PR head ref and the captured base commit into the configured clone, verifies the head SHA, and creates a detached worktree under its app-data directory. This changes Git metadata but does not check out or edit the original working tree. Git hooks are disabled for these operations. Temporary `refs/orbit/*` refs are deleted from your clone as soon as the worktree exists.

Each review gets its own Codex thread and working directory. Setup and agent commands use the Codex **workspace-write** sandbox, with network access disabled unless configured for that repository.

This is a **worktree plus the Codex OS sandbox**, not a VM or a Docker/devcontainer environment. It uses host-installed tools, the repository's manifests/lockfiles, and the user-configured setup command. Worktrees do not copy untracked dependencies or secrets from the original clone. Codex's local configuration, provider access, sandbox support, and managed policies still apply; this app does not claim to hide all host files or all inherited integrations from Codex. Only configure repositories and setup commands you intend to run.

> [!WARNING]
> Because the agent can read files outside its worktree, a pull request can plant instructions in its diff that try to get local secrets quoted in the report. That is why automatic review and publication are restricted to repository insiders: pull requests from a branch in the repository itself, by an owner, organization member, collaborator, or trusted account. The setup command can also execute code from the pull request, such as package install scripts, so prefer options like `--ignore-scripts`. With network access enabled for a repository, both setup and the agent can reach the network.

A report is never published automatically when it contains text matching a well-known credential format, and publishing one manually requires an explicit confirmation. The scan is a best-effort backstop, not a guarantee: secrets without a recognizable format are not detected.

## Persistence and recovery

- State is atomically replaced in `state.json` under Tauri's app-data directory (typically `~/.local/share/dev.orbit.review` on Linux). Invalid state is left untouched.
- Active runs become visibly interrupted after restart instead of being silently retried.
- A state-write failure turns Autopilot off; after resolving it, re-enable Autopilot to resume automatic reviews.
- Threads and worktrees are retained for follow-ups. Orbit removes a review's worktree automatically once its pull request is no longer open in a connected repository (merged, closed, or the repository was disconnected), and when a review fails or is stopped before it starts; the chat stays readable, but follow-ups then need a new review.
- Manual worktree removal checks registered ownership and eligibility under the Git lock, persists cleanup intent before touching files, and recovers pending removals after restart. Interrupted automatic cleanup rechecks PR closure on restart, independently of initial inbox sync.

## Concurrency and locking

The engine's locks, what each protects, and the order to take them in are documented on the `Engine` struct in [`src-tauri/src/engine/mod.rs`](../src-tauri/src/engine/mod.rs). In short: one connect, sync, publication, and worktree removal at a time; Git commands against your clones are serialized; and `std` mutexes are never held across an `.await`.

Narrow updates preserve unrelated settings. Global settings and repository operations are ordered independently, so slow checkout validation cannot delay turning Autopilot off. Filesystem validation runs off the async workers with a bounded wait.

## Process lifecycle

On exit, Orbit closes the app-server's stdin and terminates it. Unix process-group termination also cleans up its descendants. Windows uses child-process termination; Windows packaging and descendant cleanup need platform validation.

## Frontend

- The frontend has no generic shell/filesystem/RPC endpoint. Native commands are narrowly scoped (see the allowlist in `src-tauri/src/lib.rs`); CSP restricts network access, raw Markdown HTML is not executed, and remote Markdown images are not fetched.
- Report export is generated and written in Rust; the frontend supplies only a review ID.
- The UI uses Astryx 0.6.3 with its neutral theme. Astryx core, theme, CLI, and the StyleX peer are pinned; the compiled theme is imported after the core reset and component CSS. The UI is dark-only, uses local system fonts, and makes no font or asset requests to external hosts.
- Settings, repositories, review loops, and chat load on demand. Pages use Astryx shell, layout, controls, dialogs, chat, and Markdown; only the diff's line grid remains custom (`DiffLines` in `src/Files.tsx` explains why). `src/styles.css` contains viewport framing, system-font tokens, compact chat docking, and diff styling. The backend, command allowlist, store behavior, and browser demo adapter are independent of the component layer.
- Astryx component guidance is generated in [`AGENTS.md`](../AGENTS.md). Before changing UI, use `npx astryx component <Name>` for the installed API.

## Design notes

The supplied architecture document was used as a recommendation. This implementation corrects the protocol lifecycle and event names, uses existing installed CLIs instead of bundling a Codex binary, and uses Astryx with its neutral theme.

Not included in this version: macOS/Windows installers, signing, auto-update distribution, and container-backed environments. Native prerequisites vary by OS.

## Verification

Controllable app-server fixtures verify model migration, reconnect recovery, immutable queued selections, exact response checks, persistence before inference, and legacy chat pinning without live inference or GitHub publication. A fake `gh` (Unix only) drives the GitHub paths. Component tests cover explicit Settings choices, the legacy selector, Astryx dialog lifecycle and shortcut isolation, guarded composer submission, and Markdown safety (images dropped, links rendered as text, no HTML execution or autolinking). Tests also exercise queue limits, cancellation, publication retry/link rendering, stale file responses, navigation shortcuts, autosave failures/serialization, status/send eligibility, repository validation, worktree cleanup contention/containment/recovery, Markdown export, streamed reports, RPC failures, persistence, and the instance lock. Rust filesystem tests use disposable repositories and state.

Native Linux GTK Save As checks on a virtual display cover successful export, cancellation, overwrite confirmation, and extensionless filename confirmation using disposable review data. Browser checks cover search, review/chat actions, publication previews, changed files, repository configuration, and responsive navigation. Real authentication and model discovery were verified; live inference and posting are not part of the read-only diagnostic (`npm run diagnose`), which uses a temporary state directory.

## References

[Codex app-server](https://developers.openai.com/codex/app-server), [Tauri Rust commands](https://v2.tauri.app/develop/calling-rust/), [GitHub pull requests](https://docs.github.com/en/rest/pulls/pulls), [GitHub pull request reviews](https://docs.github.com/en/rest/pulls/reviews), [GitHub issue/PR comments](https://docs.github.com/en/rest/issues/comments), [GitHub commit statuses](https://docs.github.com/en/rest/commits/statuses).
