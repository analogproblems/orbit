# CLAUDE.md

Orbit is a Tauri 2 desktop app that reviews GitHub pull requests in a loop: it reviews each new head with Codex, Claude or Antigravity in a detached worktree, posts findings as a plain PR review, and repeats until nothing blocking is left. The React frontend renders a snapshot of the Rust engine's state and sends named commands; the engine does everything else (polling GitHub, queueing, worktrees, reviewers, publishing, saving state).

UI rules for the Astryx design system live in AGENTS.md and apply to every UI change:

@AGENTS.md

## Commands

```sh
npm run check      # Everything: lint, build, frontend tests, Rust tests, desktop build check. Run before every push; CI runs it too.
npm run lint       # ESLint, Prettier --check, rustfmt --check, Clippy with -D warnings
npm run format     # Prettier and rustfmt (fixes what lint complains about)
npm test           # Vitest (jsdom); one file: npx vitest run src/Live.test.tsx
npm run test:rust  # cargo test --no-default-features (no Tauri needed); one: npm run test:rust -- review_loop
npm run dev        # Browser demo with sample data at http://127.0.0.1:1420
npm run desktop    # The real desktop app (needs a display)
npm run server     # Orbit without a window (orbit-server), serving the browser page
```

`npm run check:desktop` and Clippy build the `desktop` feature, which needs Tauri's Linux libraries (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev` and friends); the [dev container](.devcontainer/) and the [CI workflow](.github/workflows/ci.yml) install them. Prettier also checks Markdown, so run `npm run format` after editing docs.

## Layout

- `src/`: React 19 + TypeScript + Zustand. `store.ts` holds state and `action(command, args)`, which goes to Tauri `invoke` on the desktop, to `/api/<command>` in a browser using remote access (`remote.ts`), or to `demoAction` in the browser demo. `types.ts` mirrors the Rust types.
- `src-tauri/src/`: the Rust side. `lib.rs` is the Tauri command allowlist and lifecycle; `server.rs` (with `bin/orbit-server.rs`) runs the engine with no window, for a computer that stays on; `engine/` is the engine, one file per area (see the doc comment on `Engine` in `engine/mod.rs`); top-level files hold logic that doesn't need the engine (reviewer CLIs, GitHub, review loop, Bit, ledger, scans, remote access).
- `docs/user-guide.md` (every control and rule, for users), `docs/architecture.md` (how it works inside, with a [code map](docs/architecture.md#code-map)), `docs/screenshots/` (README images, taken from the browser demo).
- `test-fixtures/`: JSON lists shared by both sides (Bit's options, finding categories, report labels) and copies of real PR comments for the ledger parsers.
- `.claude/skills/answer-orbit-reviews/`: the skill Orbit hands to authors' agents. Tests parse its examples with the real reply parser, so edit it and `engine/claims.rs` together.

## Adding or changing a native command

A command is wired in four places, and missing one fails in only one mode:

1. The engine method, plus a `#[tauri::command]` wrapper and its entry in `generate_handler!` in `src-tauri/src/lib.rs`.
2. `COMMANDS` and `dispatch` in `src-tauri/src/remote.rs`, or `DESKTOP_ONLY` if a browser must never run it.
3. The demo's behaviour in `demoAction` in `src/store.ts`, so `npm run dev` still works.
4. Types in `src/types.ts`, matching the Rust `serde` names (camelCase).

## Conventions

- **Tests sit beside the code.** Rust: `foo_tests.rs` next to `foo.rs`, included with `#[cfg(test)] #[path = "foo_tests.rs"] mod tests;` (or `mod foo_tests;` in `engine/mod.rs`). Frontend: `Foo.test.tsx` next to `Foo.tsx`. Fake CLIs (`*_fixture.cjs`, a fake `gh` on Unix) drive the reviewer and GitHub paths; nothing in the tests calls a real model or GitHub. The one exception to "beside the code" is `src-tauri/tests/server.rs`, which starts the real `orbit-server` program.
- **Lists that both sides know** (Bit's colours, hats and actions, finding categories) live once in `test-fixtures/*.json`, and tests on both sides check against them. Change all three together.
- **Locks.** `std` mutexes are never held across an `.await`. When taking more than one lock, follow the order in the table on `Engine`.
- **State** is one `state.json`, rewritten whole on each change and pruned by `engine/prune.rs`. New fields need `#[serde(default)]` so older state files still load.
- **UI text is plain language**: short sentences, no jargon, the same words the user guide uses. The UI is dark-only and makes no requests to external hosts.
- **Only the diff grid, the Live chart and Bit's picture are hand-drawn** (`DiffLines` in `Files.tsx`, `ActivityChart` in `Live.tsx`, `Bit.tsx` with the orb looks in `orbs.ts`). Everything else uses Astryx components and tokens.
- **Docs follow behaviour.** A change users can see updates `docs/user-guide.md`; a change to internals or an invariant updates `docs/architecture.md`. The README is a short showcase: link to the guide rather than adding detail there.

## Things that must stay true

- Orbit never approves, requests changes or merges; a published review is always a plain comment.
- Automatic review and publication only cover non-draft PRs from a branch in the repository by an owner, member, collaborator or trusted account, never forks.
- A report that matches a credential format is never published automatically.
- Reviewers stay isolated: Codex runs in its workspace-write sandbox with its extras off (`extras.rs`); Claude runs with settings, MCP and plugins off, its sandbox required and a tool allow list (`claude.rs`); Antigravity runs with `--sandbox` and is refused if its settings allow more (`antigravity.rs`). Don't loosen these flags.
- Dev container recipes come from the base branch, never the PR, and containers get no host folders, sign-ins or network unless allowed.
- The hidden markers in published reviews (`orbit-review`, `orbit-verdict`, `orbit-finding`, `orbit-data`) and in Orbit's CI comments and issues (`orbit-ops`, `orbit-ops-note`, and `orbit-ready` for whether a pull request is clear to merge), and the reply grammar (`Fixed F2 in <sha>` / `Declined F2: <reason>` for reviews, `Taking I2` / `Fixed I2 in <sha>` / `Declined I2: <reason>` / `Need a person on I2: <question>` for CI), are read back from older comments, so keep them compatible.
- Every GitHub write for a CI problem goes through the journal in `engine/ops.rs`: recorded and saved before it is sent, and anything new looked for on GitHub first, so nothing is posted twice.
- Orbit never pushes to an agent's branch or edits its code, never adds a push guard (required reviews or checks) you didn't ask for, and never calls a problem fixed before it has seen its checks pass.
- PR and agent text is untrusted: render it through `SafeMarkdown`, which drops images and HTML.

## Releases

Bump the version in `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (plus their lockfiles, `package-lock.json` and `src-tauri/Cargo.lock`), merge, then tag `master` as `vX.Y.Z` and push the tag. `.github/workflows/release.yml` checks the versions match and builds a draft release for Linux, macOS and Windows. Details are in [CONTRIBUTING.md](CONTRIBUTING.md#making-a-release).
