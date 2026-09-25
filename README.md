# Orbit

A local desktop workspace for routing GitHub pull requests to Codex review agents. Built with Tauri 2, Rust, React 19, TypeScript, Zustand, and the Astryx design system.

For operating instructions checked against the implementation, see the [user guide](docs/user-guide.md).

## Run

Install Node.js 22.12+ (or a newer supported version), current stable Rust (tested with 1.98), Git, the GitHub CLI, the Codex CLI, and the [Tauri native prerequisites](https://v2.tauri.app/start/prerequisites/). The app uses the `gh`, `git`, and `codex` executables on your `PATH`.

```sh
npm install
gh auth status
codex login status
npm run desktop
```

If needed, sign in with `gh auth login` and `codex login` first. Orbit does not request or store copies of your tokens. GitHub CLI and Codex manage their own authentication. On desktop systems where GUI apps have a different `PATH` or keyring environment, launch Orbit from the same terminal where those commands work.

`npm run dev` opens a **browser-only interactive demo** at `http://127.0.0.1:1420`. It has clearly labeled sample data and simulated reviews, stored separately in browser local storage. Browser mode cannot access your repositories, run Codex, or publish real comments. Stop an existing preview before running `npm run desktop`, since both use port 1420.

## Connect your repositories

1. Open **Settings** and verify GitHub and Codex are connected. Models and supported reasoning efforts come from your installed Codex app-server. Orbit resolves empty settings to an explicit model and supported default effort, saves them, and shows the selection in Settings before enabling reviews.
2. Open **Repositories → Add repository**. Pick the repository from the searchable list of repositories your GitHub account can access (owned, collaborator, or organization; archived repositories are hidden), or type `owner/repository`. Each entry shows whether it is private or public. Then enter the **absolute path** to an existing local clone. **Save repository** verifies its working tree and matching GitHub `origin` and stores the canonical checkout root, without fetching or running setup. Errors stay in the dialog.
3. Optionally configure an environment setup command, such as `npm ci --ignore-scripts --cache .cache/npm`. It runs in the new worktree through Codex's sandboxed `command/exec`, with a five-minute timeout. Enable sandbox network access if setup must download dependencies. The command uses `/bin/sh` on Unix and PowerShell on Windows.
4. Sync the inbox, select a PR, and click **Start review**. The chat shows progress, execution logs, approval requests, and the final review. Continue the same conversation with a follow-up.
5. Preview and publish the report using **Publish review**. Orbit posts an ordinary PR comment with the reviewed head/base SHAs, model, effort, and report. A failed request keeps the preview open for retry; confirmed comments have a **View comment on GitHub** action.

The inbox shows every open PR in your connected repositories, including PRs opened by bots; use its filters to narrow the view to reviews requested from you or PRs you authored. Each sync lists each connected repository's open PRs with one request and fetches an individual PR only when it is new or has changed. A repository that fails to sync keeps its last-known PRs and shows an error while the others continue, and autopilot only acts on repositories that synced successfully in the current session. Draft PRs can be reviewed manually.

## Deterministic model selection

Every new review pins an explicit model and effort when queued. Orbit validates that pair against the connected account's catalog before enqueueing and preparation. Changing workspace Settings affects future runs only. Model selection uses the catalog's `model` identifier, not its display ID.

When either workspace value is empty, Orbit reads the effective Codex configuration. An empty model uses the configured model when it is in the catalog; otherwise it requires exactly one catalog default. An empty effort uses `config.model_reasoning_effort` when the selected model supports it, otherwise the model's supported default effort. If only effort is missing and `config/read` fails, Orbit falls back to that supported default; a missing model still requires a successful config read. An unavailable or unsupported fallback blocks execution. Both values are saved together against the current workspace state, and the notice identifies each value's source and explains any fallback. Explicit selections are never silently replaced. Discovery, unsupported selections, ambiguous defaults, or failed saves block new reviews with a Settings explanation. **Reconnect** refreshes discovery even while Codex is connected. The last verified catalog stays usable during refresh; a failed refresh blocks execution until discovery succeeds. The resolution notice clears on the next successful connection. Unrelated settings and disabling automation remain available.

Orbit sends the pinned model, matching `review_model`, and reasoning effort on thread creation. It requires Codex's returned model and effort to match exactly and persists the confirmation before `review/start`. Every follow-up resumes with its pinned pair, verifies and saves the confirmation, and passes the same model and effort on `turn/start`. Missing/null or mismatched responses stop execution. Threads created during a stopped or rejected preparation are archived; if archiving fails, Orbit tries to unsubscribe and records the thread ID in the chat. Orbit does not request provider fallback or modify your Codex configuration. The compatibility baseline is Codex CLI **0.156.1** and the [official app-server protocol](https://developers.openai.com/codex/app-server).

Older chats with complete original selections reuse and verify those values. Chats missing either value offer **Choose chat model** beside the composer. Saving this one-time choice does not send a message; it pins future follow-ups without rewriting the original report. Orbit unsubscribes its idle thread before the first legacy resume so Codex can apply overrides. If another subscriber prevents that change, response verification stops the turn.

The UI separates report selection from chat selection. Exports and new comments identify confirmed report settings, or label historical values **requested, unverified** / **not recorded**. Later chat confirmations never retroactively verify a report. Questions appear immediately with **Sending…** while Orbit resumes and verifies the chat. Failed or stopped attempts remain visible as **Send failed** or **Sending interrupted**; successful sends record their confirmed selection on the same message. A failed follow-up preserves the report, publication, and composed text for retry.

## Automatic and manual control

Automation starts **off**. Enable individual repositories, then turn on **Autopilot** to enqueue new PR revisions in those repositories. Each `(repository, PR number, head SHA, base SHA)` runs at most once automatically, including failed attempts. Manual **Review again** is the explicit retry path. Drafts are excluded from automatic review, as are pull requests opened from a fork or by an author who is not an owner, organization member, or collaborator of the repository. Those can still be reviewed manually. Pull requests opened with your own `gh` login from a branch in the repository count as yours. Accounts that open pull requests for you, such as a coding agent's bot (`claude[bot]`), can be added under **Settings → Trusted accounts** or with the **Trust** button on a skipped pull request. Their pull requests are then treated like yours, provided they come from a branch in the repository rather than a fork. Only trust bots that act for you: anyone who can direct a trusted bot can get its pull requests reviewed and published automatically.

**Automatically publish findings** is a separate opt-in. Only automatically started reviews are eligible for automatic commenting, and only for pull requests opened from the repository itself (not a fork) by an owner, organization member, collaborator, or trusted account. A report is also never published automatically when it contains text matching a well-known credential format (GitHub, OpenAI/Anthropic, AWS, Slack, Google, Stripe, and npm tokens, private keys, and JSON web tokens). Held reports show the reason and stay available for manual publication. Publishing a credential-shaped report manually requires an explicit confirmation. The scan is a best-effort backstop, not a guarantee: secrets without a recognizable format are not detected. Turning autopilot off pauses queued automatic reviews; already running reviews continue and can be stopped individually. Turning automatic publication off leaves reports available for manual publication. A request already sent to GitHub cannot be recalled by turning the switch off.

The queue supports one to four concurrent reviews. Polling and review orchestration run in Rust, independent of which view is open. Orbit must remain running; this version does not install a background daemon or GitHub webhook.

## Settings, reports, and storage

Settings switches and selections autosave with **Saving**, brief **Saved**, and local error/**Retry saving** feedback. After a failed write, controls show the active values; the unapplied request and error remain beside Retry across page navigation. Changing models saves the model and its explicit supported default effort in one update. A failed model change must be retried or replaced before selecting effort. Trusted accounts save on Add, Enter, or removal; partly typed names remain local. Repository dialogs retain **Save repository**. Narrow updates preserve unrelated settings; global settings and repository operations are ordered independently so slow checkout validation cannot delay turning Autopilot off. Filesystem validation runs off the async workers with a bounded wait.

Reports distinguish **Ready to publish**, **Outdated report**, and **Saved report**, while published runs retain their status. The chat explains why a follow-up cannot send and retains composed text while waiting. Changed files have a retry action and reload when head or base changes. Use Ctrl/Cmd+K to open and focus inbox search and Ctrl/Cmd+, for Settings; open dialogs keep navigation shortcuts from taking over.

**Settings → Storage → Inspect storage** estimates saved-state and worktree file sizes without following symlinks. Scans happen on request, report incomplete results, and list unrecognized entries without making them removable. Select inactive runs, preview the estimated space and loss of follow-ups, then confirm **Remove worktrees**. Rust checks registered ownership and eligibility under the Git lock, persists cleanup intent, and recovers pending removals after restart. Failures remain retryable. Reports, chats, publication metadata, and automatic-review deduplication are preserved; there is no age-based retention or history deletion.

Confirmation closes immediately while removal runs in the background; progress and results survive navigation. Interrupted automatic cleanup rechecks PR closure on restart, independently of initial inbox sync. Automatic sweeps skip busy Git operations. Confirmed manual removal can wait for Git and continues even if its PR is open. If Orbit cannot save the removal record, it keeps the files and shows an error: free space or restore write access to its data directory, then retry.

If files were removed but saving the completion record fails, Storage reports success with a separate warning. Orbit retries saving and reconciles the durable removal record after restart; the UI does not offer to remove the same worktree again.

**Export report** opens a native Save As dialog for inactive runs with a report. The Markdown includes PR/commit/model/effort/publication metadata and excludes chats and logs. Rust generates and writes the report; the frontend supplies only a review ID. The browser demo simulates storage/cleanup and downloads Markdown.

If you enter an extensionless filename, the native dialog asks you to confirm the name with `.md` added, including any overwrite confirmation. Orbit writes only the final destination chosen in that dialog.

## How isolation works

Orbit fetches GitHub's PR head ref and the captured base commit into the configured clone, verifies the head SHA, and creates a detached worktree under its app-data directory. This changes Git metadata but does not check out or edit the original working tree. Git hooks are disabled for these operations.

Each review gets its own Codex thread and working directory. Setup and agent commands use the Codex **workspace-write** sandbox, with network access disabled unless configured for that repository. Command and file-change escalation requests are displayed for a person to approve or deny; unknown interactive requests are not automatically granted. The app explicitly routes approvals to the user even when a different reviewer is configured globally.

This is a **worktree plus the Codex OS sandbox**, not a VM or a Docker/devcontainer environment. It uses host-installed tools, the repository's manifests/lockfiles, and the user-configured setup command. Worktrees do not copy untracked dependencies or secrets from the original clone. Codex's local configuration, provider access, sandbox support, and managed policies still apply; this app does not claim to hide all host files or all inherited integrations from Codex. Only configure repositories and setup commands you intend to run.

Because the agent can read files outside its worktree, a pull request can plant instructions in its diff that try to get local secrets quoted in the report. That is why automatic review and publication are restricted to repository insiders, as described above. The setup command can also execute code from the pull request, such as package install scripts, so prefer options like `--ignore-scripts`. With network access enabled for a repository, both setup and the agent can reach the network.

## Reliability and local data

- Codex uses newline-delimited app-server messages over stdio: `initialize` → client `initialized` → `thread/start` → `review/start`; follow-ups use `thread/resume` and `turn/start`. Streaming and final review items are routed by thread and turn IDs.
- Before publication, Orbit checks that the PR is still open and both SHAs still match. GitHub does not offer an atomic compare-and-comment endpoint, so a commit can still arrive in the small interval between that check and the POST; comments always identify the exact reviewed SHAs.
- A stable marker in each comment reconciles ambiguous retries after a timeout or restart. A local instance lock prevents two Orbit processes from using the same state directory concurrently. This is local deduplication, not coordination across multiple computers.
- State is atomically replaced in `state.json` under Tauri's app-data directory (typically `~/.local/share/dev.orbit.review` on Linux). Active runs become visibly interrupted after restart instead of being silently retried. Invalid state is left untouched. A state-write failure turns Autopilot off; after resolving it, re-enable Autopilot to resume automatic reviews.
- Threads and worktrees are retained for follow-ups. Reviews remain accessible through Activity when a PR leaves the inbox. Orbit removes a review's worktree automatically once its pull request is no longer open in a connected repository (merged, closed, or the repository was disconnected), and when a review fails or is stopped before it starts; the chat stays readable, but follow-ups then need a new review. Temporary `refs/orbit/*` refs are deleted from your clone as soon as the worktree exists.
- On exit, Orbit closes the app-server's stdin and terminates it. Unix process-group termination also cleans up its descendants. Windows uses child-process termination; Windows packaging and descendant cleanup need platform validation.
- The frontend has no generic shell/filesystem/RPC endpoint. Native commands are narrowly scoped; CSP restricts network access, raw Markdown HTML is not executed, and remote Markdown images are not fetched.

## Development and verification

```sh
npm run check      # Lint, TypeScript, production frontend build, UI-state tests, Rust tests, desktop build check
npm run lint       # ESLint, Prettier, rustfmt, and Clippy with warnings as errors
npm run diagnose   # Live read-only GitHub/Codex connection check; no inference or comments
npm run format     # Prettier and rustfmt
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri -- build --no-bundle  # Produce a native executable
```

The diagnostic uses a temporary state directory. `src-tauri/target/release/orbit` is the Linux executable after building. Native prerequisites vary by OS; macOS/Windows installers, signing, auto-update distribution, and container-backed environments are not included in this initial version.

Controllable app-server fixtures verify model migration, reconnect recovery, immutable queued selections, exact response checks, persistence before inference, and legacy chat pinning without live inference or GitHub publication. Component tests cover explicit Settings choices, the legacy selector, Astryx dialog lifecycle and shortcut isolation, guarded composer submission, and Markdown safety (images dropped, links rendered as text, no HTML execution or autolinking). Tests also exercise queue limits, cancellation, publication retry/link rendering, stale file responses, navigation shortcuts, autosave failures/serialization, status/send eligibility, repository validation, worktree cleanup contention/containment/recovery, Markdown export, streamed reports, RPC failures, persistence, and the instance lock. Rust filesystem tests use disposable repositories and state; native dialog presentation requires an interactive desktop check. Browser checks cover search, review/chat actions, publication previews, changed files, repository configuration, and responsive navigation. Real authentication/model discovery were verified; live inference and posting are not part of the read-only diagnostic.

## Architecture

| Area                                                    | Implementation                                                                                                                     |
| ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| UI shell and theme                                      | `src/App.tsx`, `src/main.tsx`, `src/styles.css`                                                                                    |
| Workflow UI                                             | `src/Inbox.tsx`, `src/Detail.tsx`, `src/Chat.tsx`, `src/Files.tsx`, `src/Repositories.tsx`, `src/Settings.tsx`, `src/Activity.tsx` |
| Typed application state and native bridge               | `src/types.ts`, `src/store.ts`                                                                                                     |
| Separate interactive demo                               | `src/demo.ts` and the demo adapter in `src/store.ts`                                                                               |
| Queue, worktrees, orchestration, persistence            | `src-tauri/src/engine.rs`                                                                                                          |
| GitHub discovery, PR details, files, comments           | `src-tauri/src/github.rs`                                                                                                          |
| Codex process and bidirectional RPC                     | `src-tauri/src/rpc.rs`                                                                                                             |
| Storage inspection, ownership checks, report generation | `src-tauri/src/storage.rs`                                                                                                         |
| Storage and report actions                              | `src/Storage.tsx`, `src/ReviewActions.tsx`                                                                                         |
| Native command allowlist and lifecycle                  | `src-tauri/src/lib.rs`                                                                                                             |

The supplied architecture document was used as a recommendation. This implementation corrects the protocol lifecycle and event names, uses existing installed CLIs instead of bundling a Codex binary, and uses Astryx 0.6.3 with its neutral theme. Astryx core, theme, CLI, and the StyleX peer are pinned; the compiled theme is imported after the core reset and component CSS. The UI is dark-only, uses local system fonts, and makes no font or asset requests to external hosts. CSP remains unchanged.

Protocol references: [Codex app-server](https://developers.openai.com/codex/app-server), [Tauri Rust commands](https://v2.tauri.app/develop/calling-rust/), [GitHub pull requests](https://docs.github.com/en/rest/pulls/pulls), [GitHub issue/PR comments](https://docs.github.com/en/rest/issues/comments). The integration was checked against the locally installed Codex CLI 0.155.1 schema.

Astryx component guidance is generated in `AGENTS.md`. Before changing UI, use `npx astryx component <Name>` for the installed API. Pages use Astryx shell, layout, controls, dialogs, chat, and Markdown; only the line-numbered diff remains custom. `src/styles.css` contains viewport framing, system-font tokens, compact chat docking, and diff styling. The backend, command allowlist, store behavior, and browser demo adapter are independent of the component layer.
