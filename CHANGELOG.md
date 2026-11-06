# Changelog

What changed in each Orbit release. Download the installers from the [Releases page](https://github.com/analogproblems/orbit/releases), and see the [user guide](docs/user-guide.md) for how each part works.

## 0.3.0

Orbit now looks after more than reviews. It watches each repository's CI, security alerts and releases, tells the project's agent what needs fixing, and can keep running on a computer that stays on.

### Operations

A new **Operations** page, below **Live** in the sidebar, shows what's failing and what needs you across every connected repository.

- **CI**: Orbit reads the checks on each default branch and open pull request, says why each one failed from the job's log, and names the commits that broke a default branch.
- **Telling agents**: while Bit is on, Orbit tells the project's agent on GitHub when its CI fails, and reads its answers (`Taking I2`, `Fixed I2 in <sha>`, `Declined I2: <reason>`, `Need a person on I2: <question>`).
- **Bad luck**: Orbit reruns a job once when it failed for a reason that isn't the code's fault, such as a lost runner or a network error.
- Orbit says when a pull request's checks passed before its base moved on, and when a check fails on the default branch too.
- **Clear to merge**: each pull request Orbit reviews says whether it's clear to merge, or the first thing in the way. Orbit tells the agent, and nudges whoever owes the next step on one that's stuck.
- **Repositories with no CI** get a note, and Bit can ask the agent to add some.
- **Runners and storage**: self-hosted runners, runs waiting for one, and Actions caches and artifacts near their limit.
- **Security**: Dependabot, code scanning and secret scanning alerts (a secret's value is never read), risky workflow settings (with actionlint and zizmor too when they're installed), security settings that drift from a baseline with **Fix** to turn one on, and Dependabot's pull requests with **Ask to rebase**. The agent hears about alerts in private repositories.
- **Releases**: releases, drafts and version tags, version files that disagree, a tag on the wrong commit, the release build of the newest tag, and releases due on a milestone date. With your tap, Orbit can push a missing tag or publish a draft once its build passes.

### Bit

- **Morning brief**: once a day, at the time and in the time zone you choose, Bit sums up what happened. A quiet day sends nothing.
- Bit can look things up on GitHub and read what changed before it answers, and **Ask Bit why** asks it about a failing check for you.
- **Playbooks**: every problem lists the steps to done and who does each, ticked from what Orbit reads.
- **How it turned out**: Orbit checks whether what it did worked, such as a rerun that passed or a fix that held, and learns lessons from things that happen more than once.
- **Quick calls**: a small model on Ollama can say whether a failure looks like bad luck, and Orbit keeps score of who was right.
- **Second look**: before Bit posts in a repository where you aren't an admin, a model checks the post first.
- **Orbit itself**: Orbit checks its own health, such as low disk space, worktrees left behind, writes to GitHub that never settled and Codex being disconnected, and lists what's wrong.
- **Bit's own questions**: Bit can ask whoever answers your questions one question of its own (off to start with), and Orbit scores how the answers turned out.
- 16 orb looks to pick from.

### Always-on Orbit

- `orbit-server` runs Orbit with no window, on a computer that stays on. `orbit-server install` keeps it running as a service (systemd on Linux, a launch agent on macOS).
- The desktop app can show an always-on Orbit instead of running its own, and **Move to it** carries your repositories, settings and history across.
- Only one Orbit posts on a pull request at a time.
- Notifications show in the browser too, and can reach your phone with Orbit's page closed.
- Orbit says when a server has no password store for API keys.

### Reviews and scans

- Claude and Antigravity can run security scans, with no OpenAI sign-in.
- **Run setup and checks in Orbit's sandbox**: a Claude or Antigravity repository can run its setup and checks before each review, without Docker.
- Orbit says when a Claude or Antigravity review can only read the code and can't run it.

### Fixes

- Closed pull requests leave the saved state once nothing names them, so a busy repository no longer slows Orbit down over time.
- With **Watch CI** off, a sync that changes nothing no longer rewrites the state file.
- A workflow whose list sits at the same indent as its key, such as `tags:` with `- v*` right under it, is read correctly.
- A comment or issue GitHub refused is no longer searched for as if it might have posted.
- Orbit says when a default branch has failed since its first commit, rather than that no commit passed before it.
- `orbit-server uninstall` finishes removing a service that systemd no longer knows.

### For contributors

- CI runs `npm run check` on every pull request.
- `CLAUDE.md` describes the project for coding agents, and the README is now a short showcase.
