# Orbit

Orbit is a local desktop app that plays the reviewer in an author/reviewer loop on your GitHub pull requests. A coding agent or a person opens a pull request. Orbit reviews its latest commit with Codex, or with Claude or Antigravity if you choose one for the repository, in an isolated worktree, posts the findings on the pull request, and marks the commit with a verdict. The author fixes or declines each finding and pushes again. Orbit reviews the new commit, starting from its earlier findings, and the loop repeats until the pull request is clean or something needs you.

Built with Tauri 2, Rust, React 19, TypeScript, Zustand, and the Astryx design system. See the [user guide](docs/user-guide.md) for every control and the [architecture notes](docs/architecture.md) for how it works inside.

## Requirements

- Node.js 22.12 or newer, stable Rust (tested with 1.98), and Git.
- The [GitHub CLI](https://cli.github.com/) (`gh`) and the Codex CLI (`codex`, tested with 0.156.1), both signed in.
- Optional: [Claude Code](https://code.claude.com/docs) (`claude`), signed in, to review a repository with Claude instead of Codex.
- Optional: the [Antigravity CLI](https://antigravity.google/docs/cli/install/) (`agy`), signed in, to review a repository with Antigravity.
- The [Tauri native prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.
- Optional: [Docker](https://docs.docker.com/get-started/get-docker/), running, and the [Dev Container CLI](https://github.com/devcontainers/cli) (`npm install -g @devcontainers/cli`), to run a repository's setup and checks in its dev container.

Orbit runs `gh`, `git`, `codex`, `claude`, and `agy` from your `PATH` and uses their own sign-ins. It never asks for or stores your tokens.

## Download

Installers for Linux (`.deb`, `.AppImage`, `.rpm`), macOS (`.dmg`, for Apple silicon and Intel) and Windows (`.msi`, `.exe`) are attached to each [release](https://github.com/analogproblems/orbit/releases). The macOS and Windows builds aren't signed yet: on macOS, open it once, then choose **Open Anyway** in **System Settings → Privacy & Security**; on Windows, choose **More info → Run anyway**. You still need the CLIs listed above.

## Run from source

```sh
npm install
gh auth status      # if needed: gh auth login
codex login status  # if needed: codex login
npm run desktop
```

If a desktop shortcut can't find the CLIs or their sign-ins, launch Orbit from a terminal where they work.

`npm run dev` opens a browser-only demo at `http://127.0.0.1:1420` with sample data and simulated reviews. It cannot reach your repositories, run Codex, or post comments. Stop it before `npm run desktop`, since both use port 1420.

## Connect your repositories

1. Open **Settings** and check that GitHub and Codex show **Connected** and that a model and effort are selected.
2. Open **Repositories → Add repository**, or tap **Set up with Bit** to be walked through it. Pick the repository and enter the absolute path to an existing local clone, or leave the path empty and Orbit downloads and keeps up to date its own copy. Optionally add a setup command for new worktrees, such as `npm ci --ignore-scripts --cache .cache/npm`. The clone does not need to be up to date; turn on **Keep this clone up to date** to have Orbit fast-forward it on each sync.
3. Open **All pull requests**, select a pull request, and click **Start review**. Follow it in **Review chat**, then use **Publish review** to post it.
4. To run the loop, turn on **Automatic reviews** for the repository and **Autopilot** in the header. Turn on **Automatically publish findings** in Settings if Orbit should post without asking.

## How the loop works

- **Rounds.** Each automatic review of a pull request's current head is one round. Orbit reviews each head and base once automatically and, by default, waits for a burst of pushes to settle before starting.
- **Findings and verdict.** The reviewer reports findings with a priority from P0 (most severe) to P3. Each one has a kind, every place it shows up, a concrete case where it breaks, the evidence, a suggested fix and how to tell it is fixed, and says whether the reviewer confirmed it. Orbit's verdict is **changes requested** while any finding at or above the blocking priority (P1 by default) is open, and **clean** otherwise.
- **On GitHub.** A published round is a pull request review. Each finding gets a short ID, such as F2, that stays the same across rounds, and says whether it is **Blocking**. A finding on changed lines becomes an inline comment the author can reply to; the rest go in the review's text, followed by the reviewer's own report, folded. The review also carries a hidden `<!-- orbit-verdict … -->` line and a hidden JSON copy of every finding (`<!-- orbit-data … -->`) for agents. While automatic publication is on, the `orbit/review` commit status also carries the verdict, such as "Round 2 · 1 blocking finding". The review is always a plain comment: Orbit never approves, requests changes, or merges.
- **The next round.** When the author pushes, Codex gets the earlier findings, the commits since, and replies from the author, you, and your trusted accounts, such as `Fixed F2 in abc1234` or `Declined F2: intended`, which Orbit recognises and checks the named commit for. It marks each earlier finding fixed, still open, or declined (accepted or disputed), and reports only new problems.
- **Replies from coding agents.** A Claude Code skill, [`answer-orbit-reviews`](.claude/skills/answer-orbit-reviews/SKILL.md), teaches the author's agent to read Orbit's findings and answer them in that form. Copy it from Settings and commit it to the repositories Orbit reviews, so cloud sessions load it too.
- **When it stops.** A loop is **clean** when a round finds nothing blocking. Only two things stop Orbit reviewing new pushes: the round limit (4 by default), and **Stop loop**. **Continue loop** grants two more rounds.
- **When it needs you.** A loop shows **Needs you** when the round limit is reached, the same blocking findings stay open two rounds in a row, the reviewer disputes a declined finding, a review fails or has no findings list, an approval is pending, or a report is waiting for you to publish it. Apart from the round limit, these are alerts, not pauses: if the author pushes again, Orbit still reviews the new commit. Use **Stop loop** to pause automatic reviews of a pull request.
- **Keeping track.** The **Review loops** page lists every loop with its round and its blocking findings over time, such as `4 → 1 → 0`, under a **Needs you** list. Orbit also sends a desktop notification when something new needs you.
- **Bit.** Orbit's buddy, Bit, sits at the bottom of the sidebar. It notes each round, each clean loop, and anything that needs you, and its face shows how things are going. Ask it what needs you, what's running, or how your loops and repositories are doing, from the desktop or a browser, or type any question for Codex to answer. **Help me set up** checks your connections and walks you through connecting a repository, the setup command its lock file suggests, and automatic reviews, one tap per step, and Bit follows up with the same steps whenever a repository is connected. When one of Orbit's own controls fixes something, such as Continue loop or Sync now, Bit offers it as a button, or does it for you if you allow it. A typed question's answer can offer buttons too, including settings changes and connecting or setting up a repository (Orbit downloads its own copy), and they wait for your tap. Bit keeps a short memory between typed questions, up to 20 general lines and 10 per repository, which you can read and trim in Settings. Rename it, pick its colour and hat, or turn it off in Settings.

The [user guide](docs/user-guide.md#the-review-loop) has the full rules.

## Safety at a glance

- **Isolation, with limits.** Each review runs in its own detached Git worktree inside the Codex workspace-write sandbox, with network access off unless you enable it. This is not a VM or container: the agent can read files outside its worktree.
- **No extra Codex tools.** Codex reviews, chats and scans run without the extras you've added to Codex, such as tool servers, plugins and connected apps (scans keep only the Codex Security plugin). Orbit checks with Codex first and stops if one stays on. Your Codex settings aren't changed.
- **Prompt injection.** A pull request can plant instructions that try to get local secrets quoted in the report. That is why automatic review and publication only cover pull requests from a branch in the repository by an owner, member, collaborator, or an account you trust. Drafts and forks are never reviewed automatically.
- **Claude reviews run an allow list.** A repository reviewed by Claude runs `claude` with your own sign-in, its settings and plugins switched off, its sandbox required (on Linux, install `bubblewrap` and `socat`), and only file reading, search and a short list of Git and text commands allowed. Anything else is declined on its own and noted in the report; Claude reviews have no network access and can't run a setup command. As with Codex, the sandbox is not a full read boundary, and a Git diff driver configured in your clone can still run inside it and write to the worktree.
- **Antigravity reviews stay in Antigravity's sandbox.** A repository reviewed by Antigravity runs `agy` with your own sign-in and `--sandbox`, never skipping its permission checks. Orbit works out the change itself (with Git's diff drivers off) and gives it to Antigravity, so it needs no commands; anything that needs approval is declined. Orbit reads Antigravity's settings but never changes them, and won't start a review if they allow commands outside the sandbox, the network, MCP tools, files outside the worktree, or working without asking. Antigravity reads the repository's own `AGENTS.md` and `.agents/` rules, which the pull request controls. Antigravity reviews have no network access and can't run a setup command. This was built from Antigravity's documentation and hasn't yet been run against the real CLI.
- **Setup commands run pull request code**, such as install scripts. Prefer options like `--ignore-scripts`. With network access on, both setup and the agent can reach the network.
- **Dev containers keep pull request code in a box.** A repository can run its setup and a check command, such as `npm test`, in its dev container instead. Orbit builds the box from the base branch's `.devcontainer/` (never the pull request's), copies the pull request in, and runs them there with no folders from your computer, no sign-ins and no network unless you allow it. The reviewer gets the results and still works in its own sandbox on your computer.
- **Credential scan.** A report that looks like it contains a credential, such as a token or private key, is never published automatically, and publishing it yourself needs an extra confirmation. The scan is best effort and misses secrets without a recognizable format.
- **Approvals.** Commands and file changes that need more access wait for you to approve or deny them; unknown requests are never granted automatically. A per-repository time limit can decline them on automatic reviews.
- **Opt-in automation.** Autopilot and automatic publication start off, and a review you start yourself is never published automatically.

Read [Isolation and its limits](docs/architecture.md#isolation-and-its-limits) before enabling automatic reviews on a repository.

## Development

```sh
npm run check      # Lint, TypeScript, production build, UI tests, Rust tests, desktop build check
npm run lint       # ESLint, Prettier, rustfmt, and Clippy with warnings as errors
npm run format     # Prettier and rustfmt
npm run diagnose   # Read-only GitHub/Codex connection check; no inference or comments
npm run tauri -- build --no-bundle  # Native executable (src-tauri/target/release/orbit on Linux)
```

### Releases

Pushing a version tag builds every installer. Bump the version in `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, merge that, then tag the merge commit and push the tag:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The [Release workflow](.github/workflows/release.yml) checks the tag matches all three versions, builds on Linux, macOS and Windows, and attaches the files to a draft release. Check the draft on the Releases page, then publish it.

### Dev container

[`.devcontainer/`](.devcontainer/devcontainer.json) describes a box with everything `npm run check` needs: Node 22, stable Rust with rustfmt and Clippy, the Linux libraries Tauri needs, Git and `gh`. Open the folder in VS Code and choose **Reopen in Container**, start a GitHub Codespace, or run `devcontainer up --workspace-folder .` with the [Dev Container CLI](https://github.com/devcontainers/cli). `npm ci` runs when the container is created. The box has no display, so use `npm run check` and the browser demo (`npm run dev`, port 1420 is forwarded) there, and `npm run desktop` on your own machine.

UI code follows [`AGENTS.md`](AGENTS.md): Astryx components and tokens only. Run `npx astryx component <Name>` before using a component.

## Architecture

| Area                                                    | Implementation                                                                                                                                    |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| UI shell and theme                                      | `src/App.tsx`, `src/main.tsx`, `src/styles.css`                                                                                                   |
| Workflow UI                                             | `src/Inbox.tsx`, `src/Detail.tsx`, `src/Chat.tsx`, `src/Files.tsx`, `src/Repositories.tsx`, `src/Settings.tsx`, `src/Loops.tsx`, `src/Live.tsx`   |
| Typed application state and native bridge               | `src/types.ts`, `src/store.ts`                                                                                                                    |
| Separate interactive demo                               | `src/demo.ts` and the demo adapter in `src/store.ts`                                                                                              |
| Queue, worktrees, orchestration, persistence            | `src-tauri/src/engine/` (one file per area; see `engine/mod.rs`)                                                                                  |
| Review loop state, verdicts, and attention items        | `src-tauri/src/review_loop.rs`, `src-tauri/src/attention.rs`                                                                                      |
| GitHub discovery, PR details, files, comments           | `src-tauri/src/github.rs`                                                                                                                         |
| Pull-request history and the Decisions page             | `src-tauri/src/ledger/` (reading), `src-tauri/src/engine/ledger.rs`, `engine/writeup.rs` (background), `src/Decisions.tsx`, `src/decisionData.ts` |
| Setup and checks in dev containers                      | `src-tauri/src/devcontainer.rs`, `src-tauri/src/engine/checks.rs`                                                                                 |
| Codex process and bidirectional RPC                     | `src-tauri/src/rpc.rs`                                                                                                                            |
| Storage inspection, ownership checks, report generation | `src-tauri/src/storage.rs`                                                                                                                        |
| Storage and report actions                              | `src/Storage.tsx`, `src/ReviewActions.tsx`                                                                                                        |
| Native command allowlist and lifecycle                  | `src-tauri/src/lib.rs`                                                                                                                            |

## Contributing and license

Bug reports and ideas are welcome as [issues](https://github.com/analogproblems/orbit/issues); [CONTRIBUTING.md](CONTRIBUTING.md) explains how. Please report security problems privately, as [SECURITY.md](SECURITY.md) describes. Orbit is released under the [Apache License 2.0](LICENSE).
