# Orbit

A local desktop workspace for routing GitHub pull requests to Codex review agents. Built with Tauri 2, Rust, React 19, TypeScript, and Zustand.

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

1. Open **Settings** and verify GitHub and Codex are connected. Models and supported reasoning efforts come from your installed Codex app-server; leaving either field blank uses its defaults.
2. Open **Repositories → Add repository**. Enter `owner/repository` and the **absolute path** to an existing local clone. Its `origin` must match the same repository on github.com, over HTTPS or SSH.
3. Optionally configure an environment setup command, such as `npm ci --ignore-scripts --cache .cache/npm`. It runs in the new worktree through Codex's sandboxed `command/exec`, with a five-minute timeout. Enable sandbox network access if setup must download dependencies. The command uses `/bin/sh` on Unix and PowerShell on Windows.
4. Sync the inbox, select a PR, and click **Start review**. The chat shows progress, execution logs, approval requests, and the final review. Continue the same conversation with a follow-up.
5. Preview and publish the report using **Publish review**. Orbit posts an ordinary PR comment with the reviewed head/base SHAs, model, effort, and report.

The default inbox scope is open PRs you are involved in. Settings can narrow this to reviews requested from you or PRs you authored. Search results are paginated up to GitHub's 1,000-result search limit; exceeding it produces a visible error instead of silently truncating the inbox. Draft PRs can be reviewed manually.

## Automatic and manual control

Automation starts **off**. Enable individual repositories, then turn on **Autopilot** to enqueue new PR revisions in those repositories. Each `(repository, PR number, head SHA, base SHA)` runs at most once automatically, including failed attempts. Manual **Review again** is the explicit retry path. Drafts are excluded from automatic review.

**Automatically publish findings** is a separate opt-in. Only automatically started reviews are eligible for automatic commenting, and only for pull requests opened from the repository itself (not a fork) by an owner, organization member, or collaborator. A report is also never published automatically when it contains text matching a well-known credential format (GitHub, OpenAI/Anthropic, AWS, Slack, Google, Stripe, and npm tokens, private keys, and JSON web tokens). Held reports show the reason and stay available for manual publication. Publishing a credential-shaped report manually requires an explicit confirmation. The scan is a best-effort backstop, not a guarantee: secrets without a recognizable format are not detected. Turning autopilot off pauses queued automatic reviews; already running reviews continue and can be stopped individually. Turning automatic publication off leaves reports available for manual publication. A request already sent to GitHub cannot be recalled by turning the switch off.

The queue supports one to four concurrent reviews. Polling and review orchestration run in Rust, independent of which view is open. Orbit must remain running; this version does not install a background daemon or GitHub webhook.

## How isolation works

Orbit fetches GitHub's PR head ref and the captured base commit into the configured clone, verifies the head SHA, and creates a detached worktree under its app-data directory. This changes Git metadata but does not check out or edit the original working tree. Git hooks are disabled for these operations.

Each review gets its own Codex thread and working directory. Setup and agent commands use the Codex **workspace-write** sandbox, with network access disabled unless configured for that repository. Command and file-change escalation requests are displayed for a person to approve or deny; unknown interactive requests are not automatically granted. The app explicitly routes approvals to the user even when a different reviewer is configured globally.

This is a **worktree plus the Codex OS sandbox**, not a VM or a Docker/devcontainer environment. It uses host-installed tools, the repository's manifests/lockfiles, and the user-configured setup command. Worktrees do not copy untracked dependencies or secrets from the original clone. Codex's local configuration, provider access, sandbox support, and managed policies still apply; this app does not claim to hide all host files or all inherited integrations from Codex. Only configure repositories and setup commands you intend to run.

Because the agent can read files outside its worktree, a pull request can plant instructions in its diff that try to get local secrets quoted in the report. That is why automatic publication is restricted to repository insiders, as described above. The setup command can also execute code from the pull request, such as package install scripts, so prefer options like `--ignore-scripts`. With network access enabled for a repository, both setup and the agent can reach the network.

## Reliability and local data

- Codex uses newline-delimited app-server messages over stdio: `initialize` → client `initialized` → `thread/start` → `review/start`; follow-ups use `thread/resume` and `turn/start`. Streaming and final review items are routed by thread and turn IDs.
- Before publication, Orbit checks that the PR is still open and both SHAs still match. GitHub does not offer an atomic compare-and-comment endpoint, so a commit can still arrive in the small interval between that check and the POST; comments always identify the exact reviewed SHAs.
- A stable marker in each comment reconciles ambiguous retries after a timeout or restart. A local instance lock prevents two Orbit processes from using the same state directory concurrently. This is local deduplication, not coordination across multiple computers.
- State is atomically replaced in `state.json` under Tauri's app-data directory (typically `~/.local/share/dev.orbit.review` on Linux). Active runs become visibly interrupted after restart instead of being silently retried. Invalid state is left untouched. A persistence failure pauses scheduling.
- Threads and worktrees are retained for follow-ups. Reviews remain accessible through Activity when a PR leaves the inbox. Worktree cleanup is currently manual: after finishing a conversation, use `git worktree list` and `git worktree remove <path>` from its source clone. Do not remove a worktree while a review or chat is running.
- On exit, Orbit closes the app-server's stdin and terminates it. Unix process-group termination also cleans up its descendants. Windows uses child-process termination; Windows packaging and descendant cleanup need platform validation.
- The frontend has no generic shell/filesystem/RPC endpoint. Native commands are narrowly scoped; CSP restricts network access, raw Markdown HTML is not executed, and remote Markdown images are not fetched.

## Development and verification

```sh
npm run check      # TypeScript, production frontend build, UI-state tests, Rust tests
npm run diagnose   # Live read-only GitHub/Codex connection check; no inference or comments
npm run format
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri -- build --no-bundle  # Produce a native executable
```

The diagnostic uses a temporary state directory. `src-tauri/target/release/orbit` is the Linux executable after building. Native prerequisites vary by OS; macOS/Windows installers, signing, auto-update distribution, and container-backed environments are not included in this initial version.

Tests exercise queue limits, cancellation, automatic versus manual publication, stale revisions, streamed reports, out-of-order RPC responses, timeouts, disconnects, approval context, persistence, restart recovery, and the instance lock. Browser checks cover search, review/chat actions, publication previews, changed files, repository configuration, and responsive navigation. Real authentication/model discovery were verified; live inference and posting are not part of the read-only diagnostic.

## Architecture

| Area | Implementation |
| --- | --- |
| UI and navigation | `src/App.tsx`, `src/styles.css` |
| Typed application state and native bridge | `src/types.ts`, `src/store.ts` |
| Separate interactive demo | `src/demo.ts` and the demo adapter in `src/store.ts` |
| Queue, worktrees, orchestration, persistence | `src-tauri/src/engine.rs` |
| GitHub discovery, PR details, files, comments | `src-tauri/src/github.rs` |
| Codex process and bidirectional RPC | `src-tauri/src/rpc.rs` |
| Native command allowlist and lifecycle | `src-tauri/src/lib.rs` |

The supplied architecture document was used as a recommendation. This implementation corrects the protocol lifecycle and event names, uses existing installed CLIs instead of bundling a Codex binary, and uses a small custom design system instead of an unverified component library.

Protocol references: [Codex app-server](https://developers.openai.com/codex/app-server), [Tauri Rust commands](https://v2.tauri.app/develop/calling-rust/), [GitHub pull requests](https://docs.github.com/en/rest/pulls/pulls), [GitHub issue/PR comments](https://docs.github.com/en/rest/issues/comments). The integration was checked against the locally installed Codex CLI 0.155.1 schema.
