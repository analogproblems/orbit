# Orbit user guide

Verified against the application code on September 23, 2026, including the usability and storage changes on `codex/usability-storage` based on commit `7768c85433e71946bf2163606a53e018fb0d8e3d`. This guide describes the desktop app. The browser demo has simulated behavior and separate data.

Orbit connects open GitHub pull requests to local Codex review runs. Each run has a Git worktree and a chat. You can start reviews yourself, let Autopilot start eligible reviews, and publish completed reports as PR comments. For how Orbit works internally, including its isolation limits, see [architecture](architecture.md).

## Your first review

1. Launch the desktop app and open **Settings**. Check that **Codex CLI** and **GitHub CLI** show **Connected** and that GitHub shows the account you intend to use.
2. Open **Repositories → Add repository**. Choose a GitHub repository and enter the absolute path to its existing local clone. Leave automatic reviews off for a manual first run, then click **Save repository**.
3. Open **All pull requests**. Saving the repository starts a sync; the refresh button also syncs on demand.
4. Select an open PR and click **Start review**. Open **Review chat** to follow its progress. Expand **Execution log** entries to inspect command and setup output.
5. If a banner says approval is needed, open **Review request**, inspect the requested action, and choose **Approve once** or **Deny**.
6. When the status becomes **Ready to publish**, read the report and the findings list under it. You can ask a follow-up before publishing.
7. Choose **Publish findings** in the chat or **Publish review** in the footer. Inspect the preview, then click **Publish to GitHub** to post using your connected GitHub account.

Starting a review manually does not publish its report automatically, even when automatic publication is enabled.

After the report arrives, the chat shows **Extracting findings…** while Codex restates its findings as a list, on the same chat and with the same pinned model and effort. Each finding in the list has a severity, a kind (bug, security, data loss, compatibility, test gap, performance or other), every place it shows up, what is wrong, a concrete case where it breaks, the evidence, whether the reviewer confirmed it by reading or running the code, a suggested fix, how to tell it is fixed, and the reviewer's own notes. A reviewer leaves out what its report didn't say rather than making it up, and a finding it didn't confirm is marked **Likely, not verified**. Each finding also gets a short ID, such as F2, and the list marks the ones that block. The list also has the verdict and anything the reviewer could not check. If the reviewer says it could not finish checking the change, the run fails and nothing is published, rather than looking clean. If this step fails, the run is labeled **Unstructured run**: its report is kept and can still be published, and exports include the verdict and a findings table when there is one. Stopping the run during this step stops it like any other review.

When you review a new head of a PR that Orbit already reviewed with a findings list, the review starts from that earlier round. Codex is given the earlier findings, the commits and the size of the changes since, and comments posted on the PR since the earlier report, but only from the PR author (when Orbit trusts the PR), your own account and your trusted accounts. Comments on the diff count too. A reply in a finding's own thread is given to Codex with that finding. Orbit also recognises two kinds of reply, from the same accounts, anywhere on the PR: `Fixed F2 in abc1234` and `Declined F2: reason`, naming findings by their short IDs (several at once, such as `Fixed F2 and F3 in abc1234`, work too). In a finding's own thread, `Fixed in abc1234` or `Declined: reason` is enough. Each published review ends with a line showing this format. For each earlier finding, the reviewer is told the latest such reply and, for a fix, whether the named commit is one of the commits since the previous round, was already in the reviewed head, isn't part of the pull request, or can't be found; it still checks the code before calling a finding fixed. **Since the last round** shows what the author said, in the app, in the published review's **Author said** column and in exports. Comments from anyone else are left out and counted, and the chat says how many. Codex still reviews the whole change against the base, marks each earlier finding as fixed, still open, declined (accepted) or declined (disputed), and reports only new problems as findings. The review chat and exports show this under **Since the last round**. After a rebase or force-push the earlier findings are still checked, but the commits since are not listed.

## Launch and connect

After building it with `npm run tauri -- build --no-bundle`, launch the compiled app from the repository folder. On Linux:

```sh
./src-tauri/target/release/orbit
```

Close an existing Orbit instance before launching another. Orbit allows only one instance using the same app-data directory.

The desktop app runs the `gh`, `git`, and `codex` programs from its environment. It uses the existing GitHub CLI authentication and Codex account configuration; there is no in-app login form or token field, and Orbit does not store copies of your tokens. If a connection is missing, use the relevant login command in a terminal:

```sh
gh auth login
codex login
```

To review a repository with Claude, install [Claude Code](https://code.claude.com/docs) and run `claude` once to sign in. To review with Antigravity, install the [Antigravity CLI](https://antigravity.google/docs/cli/install/) and run `agy` once to sign in. Both are optional: Codex reviews work without them. To run a repository's setup and checks in its [dev container](#run-setup-and-checks-in-a-dev-container), install and start [Docker](https://docs.docker.com/get-started/get-docker/) and run `npm install -g @devcontainers/cli`; **Settings → Connections** shows whether both are ready.

Then use **Settings → Connections → Check again**. That action checks Claude, Antigravity and dev containers again, connects Codex and, if the connection succeeds, syncs GitHub. The inbox refresh button can sync GitHub separately. If launching through a desktop shortcut cannot find the CLIs or their sign-ins (some desktops give GUI apps a different `PATH` or keyring environment), launch the executable from the terminal where they work.

### Codex usage

At the bottom of the side navigation, a small meter shows how much of your Codex allowance is used, such as **Week 42% · resets Mon** and **5h 10%**. Each window is named by its length, and the bar follows the longest one. The gauge button next to it opens the details: your plan, each window with its reset time, limits tied to particular models, and credits if your account has them. The numbers match Codex's own usage view. Orbit reads them when it connects, updates them as Codex reports usage during reviews, and reads them again every five minutes.

When a limit is reached, the meter says **Limit reached**, and a review that stops because of it says so instead of looking broken. Start it again after the limit resets. An API key login has no usage windows, so the meter is hidden.

GitHub integration is fixed to **github.com**. There is no GitHub Enterprise hostname setting. Git fetches use the clone's `origin`, so Git must also be able to authenticate to that remote without an interactive prompt.

For development, run these commands from the Orbit checkout:

| Command                              | What it does                                                                                                                                                      |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `npm run desktop`                    | Starts the native development app and its frontend server.                                                                                                        |
| `npm run dev`                        | Starts the browser demo on port 1420. It cannot run real reviews or post real comments.                                                                           |
| `npm run tauri -- build --no-bundle` | Builds the frontend and native executable. On Linux the executable is `src-tauri/target/release/orbit`.                                                           |
| `npm run diagnose`                   | Checks Codex connectivity/model discovery, GitHub connectivity, and repository listing using temporary Orbit state. It does not start a review or post a comment. |

Source builds require installed npm dependencies, Rust/Cargo, and the native dependencies needed by Tauri. Stop an existing `npm run dev` server before using `npm run desktop`: the development server requires port 1420 to be available. The compiled executable does not need that server.

If you see **Interactive demo**, you are using simulated data. **Connect your workspace** opens instructions; it does not turn a browser session into the desktop app. **Reset sample workspace** resets only the demo.

## Appearance

Orbit uses Astryx’s neutral dark theme and system fonts, including in dropdown menus and dialogs. No font download is needed. The sidebar collapses to an icon rail in narrower windows; its expand button restores the labels. PR titles wrap in the inbox. Branch details and change counts are available in **Overview**, leaving more room for the conversation in **Review chat**.

## Connect and configure repositories

In **Repositories**, click **Add repository** or **Connect a repository**. If you'd rather be walked through it, **Set up with Bit** on the empty inbox or Repositories page opens Bit's [Help me set up](#ask-bit).

| Field or control                                       | How to use it                                                                                                                                                                                                        |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **GitHub repository**                                  | Search repository names or descriptions, then select a result. Private/public labels describe GitHub visibility.                                                                                                     |
| **Refresh repositories**                               | Reload the picker after gaining access to a repository or creating one. The list otherwise stays cached for the session.                                                                                             |
| **Use owner/name**                                     | Type a complete repository name to connect one not shown in the picker. This does not grant access.                                                                                                                  |
| **Local checkout path**                                | Enter an absolute path to an existing clone, or click **Browse…** in the desktop app to pick its folder. A literal `~` is not an absolute path. When adding a repository, you can leave it empty instead: see below. |
| **Keep this clone up to date**                         | Fast-forwards the clone's checked-out branch to GitHub on each sync. Off by default. See below.                                                                                                                      |
| **Environment setup**                                  | Optional shell command to prepare each new review worktree. Leave blank when host tools and checked-in files are sufficient.                                                                                         |
| **Run setup and checks in the dev container**          | Runs the setup command and a check command in a box built from the base branch's `.devcontainer/`. Off by default. See below.                                                                                        |
| **Check command**                                      | With the dev container on: a command such as `npm test` that runs after setup. Its result goes to the reviewer.                                                                                                      |
| **Let the reviewer run commands in the dev container** | With the dev container on: keeps the box for the review and lets Codex and Claude reviewers run their own commands in it. Off by default. See below.                                                                 |
| **Allow sandbox network access**                       | Allows network access for setup and the new review's agent environment. Off by default.                                                                                                                              |
| **Include in automatic reviews**                       | Makes this repository eligible when workspace Autopilot is also on. Off by default.                                                                                                                                  |

Click **Save repository**. With the path left empty, the button reads **Download and connect**: Orbit downloads its own copy with the GitHub CLI, signed in as you, into `clones/<owner>/<name>` in its data folder (or uses the copy already there, such as one Bit downloaded), keeps it up to date, and connects it. This works from a browser too, such as [from your phone](#use-orbit-from-your-phone-or-laptop). If you close the window or browser while it downloads, Orbit still finishes and connects the repository. A download that's cut short is thrown away, never connected. A large repository can take a while. If the download fails, the dialog shows why.

Saving a new repository or changing its name/path verifies an existing Git working tree and a matching supported GitHub `origin`, then stores the canonical checkout root. Validation does not fetch or run setup. Errors stay in the dialog so you can correct the configuration. Saving starts an inbox sync. Review preparation checks the checkout again because it may have changed. An unavailable existing checkout does not prevent disabling automation, disconnecting it, or changing unrelated Settings.

Filesystem validation runs outside the async workers, with a ten-second wait limit per filesystem check. If a mount stalls, validation reports a timeout and leaves the configuration unchanged. The operating system may finish the underlying filesystem request later; its late result does not save the repository.

The picker loads up to 1,000 accessible repository entries and omits archived repositories. It displays up to 50 unconnected search matches at a time and shows the total when more matches exist; narrow the search or enter `owner/name` directly. Repository names are treated case-insensitively and cannot be connected twice.

The clone's `origin` must match the configured GitHub repository. Accepted forms include `https://github.com/owner/name`, `git@github.com:owner/name`, and `ssh://git@github.com/owner/name`, with an optional `.git` suffix. Custom SSH host aliases do not match these forms.

The clone does not need to be up to date. Each review fetches the PR head and base commit it needs into the clone, whatever branch is checked out.

### Keep the clone up to date

Turn on **Keep this clone up to date** when the clone on disk drifts behind GitHub, for example because you mostly work elsewhere. After each sync (on the poll interval and after **Sync now**), Orbit runs `git fetch origin` in the clone and, when the checked-out branch is only behind the branch it tracks, fast-forwards it with `git merge --ff-only`. Git hooks are disabled for these commands.

Orbit never discards or rewrites your work. It leaves the clone unchanged and says why on the repository when the clone is not on a branch, the branch tracks no remote branch, the branch has local commits that GitHub does not, or tracked files have uncommitted changes. Untracked files do not block an update unless Git would have to overwrite one, in which case the update fails and the repository shows Git's error. Only the checked-out branch is moved; other local branches are left alone.

### Choose the reviewer

Each repository has a **Reviewer**: **Codex** (the default), **Claude**, or **Antigravity**. Claude reviews use the Claude Code CLI with your own Claude sign-in, and the **Claude reviews** model and effort in Settings. Claude reviews with its own built-in code review rules, then Orbit asks it, on the same conversation, to list its findings in Orbit's format. Later rounds, the review loop, publishing, inline comments and follow-up chat work the same as with Codex.

Orbit starts `claude` with your Claude settings, plugins and connectors switched off, its sandbox on, and only file reading, search, and a short list of Git and text commands (`git diff`, `log`, `show`, `status`, `blame`, `merge-base`, and `head`, `tail`, `grep`, `wc`) allowed, each inside the sandbox. Anything else, such as editing files or running tests, is declined on its own and noted in the report, so a Claude review never waits for your approval. This is an allow list, not a read-only guarantee: like Codex's, the sandbox can write to the worktree and is not a full read boundary, and a Git diff driver configured in your clone still runs when Claude shows a diff. Claude reviews have no network access and can't run a setup command, except in the repository's [dev container](#run-setup-and-checks-in-a-dev-container): clear the setup command or turn that on before choosing Claude. They only run inside Claude's sandbox; where it can't start (on Linux it needs `bubblewrap` and `socat`; native Windows has none), the review fails and says why. If a review changes the worktree, the chat says so.

Antigravity reviews use the Antigravity CLI (`agy`) with your own sign-in, and the **Antigravity reviews** model and effort in Settings. Antigravity has no review mode of its own, so Orbit gives it Orbit's own review instructions together with the change, which Orbit works out with Git (with diff drivers off). A change over 200 KB goes, whole, into a file named `.orbit-change.diff` in the review's worktree for Antigravity to read. Then, in the same conversation, Orbit asks for the findings in Orbit's format. Choose a model in Settings before the first Antigravity review: Orbit lists the models `agy models` offers and has no default.

Orbit runs every Antigravity turn with `--sandbox` and never skips Antigravity's permission checks, so anything that needs approval, such as running a command, is declined on its own and noted in the report. Commands you allowed in Antigravity's own settings run inside its sandbox, with no network and the worktree writable. Orbit reads your Antigravity settings (`~/.gemini/antigravity-cli/settings.json`) but never changes them. It won't start a review if they allow commands outside the sandbox (`unsandboxed`), the network (`read_url`, `execute_url`), MCP tools (`mcp`), file access outside the worktree (`read_file`, `write_file`), working without asking (`toolPermission` other than request review or strict), or files outside the workspace (`allowNonWorkspaceAccess`). Change them in `agy` with `/permissions`. Antigravity reads the repository's own `AGENTS.md` and `.agents/` rules, which the pull request controls. Antigravity reviews can't run a setup command, except in the repository's [dev container](#run-setup-and-checks-in-a-dev-container). Antigravity support was built from its documentation and hasn't yet been run against the real CLI.

**Settings → Connections** shows whether Claude and Antigravity are connected, and says **Not installed** or **Not signed in** when Orbit can tell. Until one is connected, its model and effort under **Settings → Reviewers** are locked and a note there says what is missing. Repositories reviewed by either wait while it isn't, and **Live** lists it under what needs you.

### Set up the review environment

Orbit creates a detached worktree at the PR's captured head commit. It uses host-installed runtimes and tools, and does not copy the original clone's untracked dependencies into the worktree. Unless the repository runs its setup and checks in its [dev container](#run-setup-and-checks-in-a-dev-container), it doesn't build a container.

Without a dev container, the setup command runs inside that new worktree through the Codex sandbox, using `/bin/sh` on Unix or PowerShell on Windows. It has a five-minute execution timeout. Its output is available in the chat's execution logs; a nonzero exit stops review preparation.

For a compatible npm project, one possible setup command is:

```sh
npm ci --ignore-scripts --cache .cache/npm
```

Choose a command appropriate to the repository. Dependency downloads need sandbox network access. Install scripts can execute PR code; `--ignore-scripts` avoids those scripts but may leave some projects needing additional setup.

The network switch controls the review sandbox, not Orbit's GitHub requests, Git fetches, or Codex's service connection. The worktree and sandbox are not a complete read-isolation boundary. The agent is instructed to review without editing tracked source, but this instruction is not a read-only filesystem guarantee.

Codex reviews, follow-up chats and the findings step run with the extra tools you've added to Codex switched off, without changing your Codex settings: every tool server in your Codex settings, plugins, connected apps, the browser, the computer, tool suggestions, memories and image generation. Codex's standard tools, such as reading files and running commands in the review's copy, stay on. A pull request can plant instructions, and an extra tool can reach outside the copy, so before each of these starts Orbit checks with Codex that no tool server is on. If one is, or Codex can't say, the review doesn't start (or the message isn't sent) and Orbit says why.

Use the configure icon on a repository card to edit its settings. Changes do not rebuild existing review worktrees; start a new run when you need a new environment. **Disconnect** removes the configuration and starts a sync. It does not delete the original clone or the saved chat history. Finished review worktrees become eligible for cleanup after the repository leaves the inbox; use **Stop** separately for running work.

### Run setup and checks in a dev container

A dev container is a recipe, kept in the repository as `.devcontainer/devcontainer.json`, for a box with everything the project needs: the same file GitHub Codespaces and VS Code use. Turn on **Run setup and checks in the dev container** for a repository, and add a **Check command** such as `npm test`, a setup command, or both. It works with every reviewer, so Claude and Antigravity repositories can have a setup command this way.

For each review, Orbit:

1. builds the box from the **base** commit's recipe with the Dev Container CLI, so a pull request can't change the box its own code runs in. The first build can take a while; later reviews reuse it until the recipe changes.
2. copies the pull request's files into a fresh box. The box gets none of your files, none of your sign-ins, and no network unless **Allow network access** is on for the repository.
3. runs the setup command (up to 15 minutes), then the check command (up to 30 minutes), and removes the box. They run as the recipe's user (its `remoteUser` or `containerUser`, else the user its Dockerfile sets), who owns the copied files. If setup fails, the checks are skipped. Stopping the review, or a new push replacing it, stops them and removes the box too.

The results show in the review's chat and under its findings, and the reviewer gets them with the end of each command's output. It reports a failure the pull request caused as a finding and mentions any other failure in its summary. A failing command doesn't stop the review; a missing recipe, a box that can't be built, or Docker not running does, with the reason in the chat.

The reviewer itself still works in its worktree on your computer, in its own sandbox. Only the pull request's setup and checks move into the box, unless you let the reviewer in too:

#### Let the reviewer run commands in the box

Turn on **Let the reviewer run commands in the dev container** (off by default) and Orbit keeps the box for the whole review instead of removing it after the checks. The reviewer gets a tool that runs a command in the box, so it can run the tests, a build or the code itself to check what it finds, and quote what it ran as evidence. Its commands run as the recipe's user in the copy of the pull request, with the repository's network setting, one at a time, for up to 10 minutes each (the reviewer can ask for up to 30). A command that runs out of time is stopped inside the box, along with everything still running there, such as a server an earlier command left going, so nothing it started can change the box afterwards. The same goes for a setup or check command that runs out of time while the box is kept. If Orbit can't confirm everything has stopped, it removes the box instead, and the reviewer is told it can't run commands there for the rest of the review. They show in the review's chat with "(in the dev container)" after them.

The reviewer keeps running on your computer with your sign-in, which never goes into the box. Changes it makes in the box don't touch the worktree it reviews. The box is removed when the review ends, is stopped or is replaced, and the tool then says so if it is used in a follow-up question. With this on, the setup and check commands are optional: without them, the reviewer simply gets the box to use.

Codex and Claude reviews can use the tool. Antigravity can't be given a tool for a single review without changing its own settings, which Orbit never does, so Antigravity reviews run without it and say so in the chat.

## Find a pull request

The inbox lists open PRs from **connected repositories**, including bot-authored PRs and drafts. It is not an account-wide search of every PR you can access. The current Settings page has no **GitHub inbox scope** control.

| Filter              | Meaning                                                                                                                             |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| **All**             | All PRs currently in the connected-repository inbox.                                                                                |
| **For you**         | PRs whose requested-reviewer list includes your connected GitHub login. Team review requests are not expanded into team membership. |
| **Authored**        | PRs authored by that GitHub login. Trusting a bot does not make its PRs appear under this filter.                                   |
| Repository dropdown | Limits the visible list to one repository.                                                                                          |
| Search              | Matches title, repository name, PR number, or author, ignoring case.                                                                |

These filters change the visible list; they do not restrict Autopilot. Use repository automation switches to control its scope.

Orbit syncs at startup and at the configured interval even in **Manual mode**. The refresh button starts an on-demand sync. A repository sync failure preserves its last-known inbox entries and reports an error in Settings; other repositories can continue syncing. More than 1,000 open PRs in a repository is reported as a sync error.

Select a PR to see its description, branches, change counts, and labels. Use the arrow beside the PR identifier to open GitHub.

**Files changed** shows the PR file list and patches fetched from GitHub, with collapsible files. The implementation requests at most 3,000 files. Some files have no patch preview. This is the PR's current GitHub diff when loaded, not a frozen diff belonging to whichever historical review you selected. A failed load shows **Retry**; retrying displays a loading state and any new failure. Changing PR, head, or base reloads the files and ignores responses from the earlier selection.

**Ctrl+K** (Windows/Linux) or **Cmd+K** (macOS) opens the inbox and focuses search from any page. **Ctrl+,** or **Cmd+,** opens Settings. These navigation shortcuts pause while a dialog is open, including the Storage removal confirmation. Dialogs keep background controls inactive and return focus to the opener when dismissed. Publishing disables dismissal until the request finishes. The displayed hints use the platform’s modifier key; arrow keys navigate tabs, filters, and selector choices.

## Run and manage reviews

**Start review** creates a run with its own chat and worktree. **Review again** creates another run; it does not replace the previous one. Only one queued or active review is allowed for a PR at a time. Manual reviews can include drafts and PRs that Autopilot would skip.

Each run captures an explicit model, effort, head commit, and base commit when queued. Orbit validates the model/effort against the connected catalog before queueing and again before preparation. Preparation fetches and verifies the head and reviews against the captured base. Codex must report the exact requested model and effort, and Orbit must save that confirmation, before review inference starts. Missing, null, or mismatched confirmation stops preparation with an error. Changing model or effort settings does not update an existing queued run.

| Status                | Meaning and next action                                                                                                              |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| **Queued**            | Waiting for a slot, or paused by automation settings. You can stop it.                                                               |
| **Preparing sandbox** | Fetching commits, creating the worktree, running optional setup, and starting the chat.                                              |
| **Reviewing**         | The review or a follow-up is running.                                                                                                |
| **Needs approval**    | A command or file-change request needs your decision. Open the approval banner.                                                      |
| **Ready to publish**  | A nonempty completed report matches the current PR revision and is eligible for publication. Check it and publish when appropriate.  |
| **Outdated report**   | The PR head or base changed. The saved report remains readable; review the current revision before publishing.                       |
| **Saved report**      | The report is retained but publication is currently unavailable, for example after a connected PR closes or during worktree removal. |
| **Publishing**        | Orbit is checking GitHub and reconciling or posting the comment.                                                                     |
| **Published**         | Orbit has confirmed a comment for this run.                                                                                          |
| **Failed**            | Open the chat error and execution logs. Correct the cause and use **Review again** when the PR is in the inbox.                      |
| **Stopped**           | The run was interrupted or cancelled. It is not automatically retried for the same revision.                                         |

Use **Stop** in the PR detail or under **All reviews** on the **Review loops** page to cancel queued work or interrupt active work. Publishing is a separate state and does not have a Stop control.

**All reviews**, folded at the bottom of the **Review loops** page, lists every run, manual ones and those whose PR has left the inbox included. Open a row to read its chat. When a PR has multiple runs, the **Review history** dropdown in its chat selects one. The sidebar shows up to five recent runs and counts the runs in progress, queued ones included, beside **Review loops**. **Ready to publish** labels use the same rules as the badges and publication buttons. Derived labels do not overwrite stored review outcomes; a published report retains **Published** even when the PR changes.

The chat shows short status notes inline. Expand **Execution log** entries for multiline setup output and recorded command output. Some output is capped, so these logs are not a complete terminal transcript. The composer stays docked at the bottom; a **Scroll to bottom** button appears when you scroll away from the latest messages.

In long inbox, loop and review lists, the first ten visible entries use live relative timestamps. The ten latest chat entries also use live relative timestamps; older entries show an absolute date and time to avoid a timer for every retained message.

### Approvals and follow-ups

The approval dialog shows the reason, working directory, and request details. **Approve once** accepts that request; **Deny** declines it. Closing the dialog leaves the request pending. A run awaiting approval still occupies a concurrency slot. Unsupported interactive request types are declined or rejected rather than automatically granted.

If **Approve once** or **Deny** fails, the approval dialog stays open and shows the error. Resolve the reported problem, then retry the same action. Both buttons are disabled while the response is pending.

After a turn finishes, type into **Message Codex**. **Enter** sends; **Shift+Enter** inserts a new line. Follow-ups use the same chat and worktree, require a free concurrency slot, and are limited to 32,000 characters. They cannot run during publication or while the worktree is being removed or has been removed. The chat explains when sending is blocked by a running turn, publication, missing chat/worktree, disconnected Codex, unresolved or unavailable model settings, or occupied slots. Composed text survives page navigation while you wait. Enter and the send button enforce the same rules. **Review again** appears when the open PR can accept a new run.

A normal follow-up adds chat messages; publishing still uses the stored review report, not the newest chat answer. Stopping or failing a follow-up preserves the prior review outcome, including an existing publication. Use **Review again** to get a new review of changed commits.

The **New review environment** panel shows current workspace settings for future runs. The overview’s **Review report** and the chat’s **Report selection** line describe the original report. The composer shows the chat's pinned selection; each new follow-up's user message records its confirmed selection. Exports and newly published comments use confirmed report metadata when available. Historical values are labeled **requested, unverified** or **not recorded**. Orbit never infers an old report's settings from current Settings or a later chat.

Every follow-up explicitly resumes the chat with its pinned model and effort. Orbit verifies Codex's response, saves the confirmation, and starts the turn with the same explicit pair. Your question appears immediately with **Sending…** while the chat resumes. Confirmation is attached to that same message after verification. A mismatch or failed save prevents sending; the question stays in the chat as **Send failed**, and the typed text remains in the composer for retry. Stopping before the send is confirmed marks the question **Sending interrupted**. An unavailable pinned selection requires reconnecting to restore access or starting a new review with a different selection.

### Older chats: one-time model selection

A chat with both original model and effort recorded reuses them for follow-ups and verifies them on resume. Its original report stays unverified.

If either value is missing, **Choose chat model** appears beside the composer. Select a catalog model and supported effort; valid workspace settings are preselected when available. **Cancel** leaves the chat unchanged. **Save chat model** persists the choice without sending your question, and a save error stays in the dialog for retry. After saving, the choice is pinned for that chat. It does not fill in or change the original report's metadata.

Before the first follow-up with that choice, Orbit unsubscribes its idle thread so Codex can apply the overrides to a loaded chat. If another subscriber prevents the change, Orbit rejects the mismatched response before sending. Reconnect and retry once the other subscriber is no longer holding the thread. Model selection is unavailable while the run is active, publishing, or its worktree is unavailable or being removed.

## Publish findings

Publishing uses the connected GitHub account. A run with a findings list is published as a GitHub pull request review. Each finding has a short ID, such as **F2**, that stays the same in every later round of the pull request and is never reused, so the author can name it in a reply. Each finding whose lines are all inside one changed part of the diff becomes an inline comment on those lines, so the PR author can reply to each finding in its own thread. A comment opens with a bold line giving the short ID, the priority, **Blocking** or **Not blocking** against the **Blocking priorities** setting the round was judged with, the kind, and **Likely, not verified** when the reviewer didn't confirm it, then the title, for example **F2 · P1 · Blocking · Bug** Off-by-one in first(). Below that come the problem, the case where it breaks, the evidence, the suggested fix, how to tell it is fixed, its other places, and the reviewer's notes. The review's own text holds the verdict with the blocking count, the summary, **Since the last round** (with each earlier finding's short ID), anything the reviewer could not check and any requests Orbit declined, and, under **Other findings**, each finding that could not go inline with its file, lines and the reason: outside the repository, file not changed in this pull request, diff too large to show, lines not in the diff, or spans more than one part of the diff. Then comes the reviewer's whole report, folded under **Codex's own report** (or Claude's or Antigravity's), so nothing the findings list left out is lost.

When Orbit can count the findings in the reviewer's report (Codex's `[P1]` titles, Claude's list, and Antigravity's numbered problems), it compares that count with the findings list and the earlier findings the reviewer still holds open. If the list has fewer, the review's text and the app say that some may have been left out.

The review's text also carries a hidden, machine-readable copy of the whole review for agents to read instead of the Markdown: an HTML comment starting `<!-- orbit-data`, holding JSON with each finding's ID, short ID, priority, whether it blocks, kind, places, every part of the form, and whether it went inline, plus the verdict, the round and the earlier findings' statuses. If the review would be over GitHub's size limit, the hidden copy is left out first, then the reviewer's report (the review says so). If GitHub refuses a comment's lines anyway, Orbit posts the review again once with every finding in its text. The review is always a plain comment review, never Approve or Request changes; Orbit's verdict is in its commit status. An unstructured run is published as a regular comment in the PR conversation, as before. Orbit never merges the PR.

The publication dialog previews the stored report. A failed request leaves the preview open with its error and **Retry publication**. A successful request closes the preview. Published run details, chats, loop rounds, and **All reviews** offer **View comment on GitHub** when the saved comment URL is available. The posted comment also includes an Orbit heading, the reviewed head and base commit IDs, and model/effort labels. For a run with a findings list, the preview lists each finding and says whether it will go on its lines or in the review's text, and why, then shows the review's text as GitHub will show it, with the reviewer's report unfolded and a line saying the hidden copy is included. It checks the PR's current diff to decide; if that check fails, the preview says so, and Orbit checks again when publishing. For an unstructured run, the preview shows the stored report. The preview is not an editor, and there is no report-editing or comment-updating control.

Before posting a new comment, Orbit requires a completed, nonempty report and rechecks that the PR is open and its head and base still match. If new commits are available, sync and use **Review again**. A GitHub change can still occur between the final check and the write; the comment identifies the exact commits reviewed.

Reports matching recognized credential formats are held from automatic publication. The check covers everything Orbit would publish: the report, the findings, and what authors said in replies Orbit recognised from the last round. Manual publication requires the **I checked the report** switch before **Publish to GitHub** becomes available. The formats are GitHub, OpenAI/Anthropic, AWS, Slack, Google, Stripe, and npm tokens, private-key headers, and JSON web tokens. This check does not detect every secret or redact matching text. Inspect the report before overriding it.

On an uncertain posting result, retry publication of the **same run**. Orbit looks for that run's marker in the PR's reviews and conversation comments before posting again. This avoids starting a fresh review merely to retry a comment. It can also confirm a comment that was posted before the PR subsequently changed. A formatted comment, review text or inline comment over Orbit's 60,000-byte threshold is refused; the error asks you to copy the report from the chat instead.

A report from a disconnected repository can still be published manually if the PR remains open and unchanged. Closed or merged PRs cannot receive a new report through Orbit. Use **View comment on GitHub** to open the confirmed comment.

## Use Autopilot

There are two review switches and a separate publication switch:

1. Enable **Automatic reviews** on each repository you want included.
2. Enable workspace Autopilot in the header, or enable **Automatically review new pull requests** in Settings. The change saves immediately.
3. If you also want comments sent without a publication preview, enable **Automatically publish findings** in Settings. The change saves immediately.

With automatic publication off, automatic reviews stop at **Ready to publish** for your inspection.

An automatic review can be queued when:

- Codex is connected, workspace Autopilot is on, and the repository's automation switch is on.
- The repository synced successfully in the latest completed poll in this session. This check governs new automatic scheduling; it is not a fresh GitHub check each time a previously queued run starts.
- The PR is not a draft and its head branch belongs to its base repository.
- GitHub identifies the author as an owner, member, or collaborator, or the login is in **Trusted accounts**. Pull requests you open with your own `gh` login from a branch in the repository qualify this way.
- There is no active run for the PR and no previous run for the same head/base pair.
- The PR's review loop is not stopped and has rounds left, and its head has not changed for one poll interval (see **The review loop** below).

**Enabling Autopilot can pick up existing eligible PRs already in the inbox**, not just PRs opened afterward. Any previous run for a head/base pair, including a manual, failed, or stopped run, prevents a new automatic attempt for that pair. A changed head or base can qualify for a new automatic run. Use **Review again** for an explicit retry.

Turning Autopilot off pauses queued automatic runs and prevents pending automatic publication while the switch is off. Already running reviews continue; stop them individually if needed. Manual queued reviews can still start. Re-enabling Autopilot allows eligible paused automatic work to continue while the app remains running. A comment request already sent to GitHub cannot be recalled by a switch.

Automatic publication is attempted when an automatic review finishes. It rechecks the workspace and repository switches, PR provenance, report contents, and revision. Turning the publication switch on later does **not** sweep existing ready reports or retry earlier posting failures; publish those manually. Manual runs remain manual even with all automation switches on.

### Trust an account

In **Settings → Trusted accounts**, enter a GitHub login such as `claude[bot]` and click **Add** or press **Enter** to save it. Removing a chip saves immediately. Partially typed account names stay local until added; navigating away preserves the input. The list allows up to 50 distinct logins, ignoring case.

For an otherwise eligible same-repository PR skipped because of its author, **Trust account-name** in the PR detail saves trust immediately. This is a workspace-wide account list, not a rule limited to that PR or repository.

Trust only accounts whose PRs you intend to review automatically. Anyone able to direct a trusted bot can supply code and instructions to its PRs. Trust never bypasses the fork exclusion, draft exclusion, or credential check, and it does not enable the automation switches by itself.

### The review loop

Each automatic review of a PR is one round of its review loop. Orbit gives every round with a findings list its own verdict: **changes requested** when a finding at or above the **Blocking priorities** setting is new or was found still open or disputed, and **clean** otherwise. Codex's own verdict is shown beside it when the two differ. Turn off **Block on findings** in **Settings → Automation** and no finding blocks: every round with a findings list is clean, and its findings carry no Blocking label. The verdict is recorded when the findings are extracted, so changing either setting affects later rounds only.

The **Review loops** page, the PR detail and the inbox show the loop's state: **Reviewing**, **Waiting for author**, **Clean**, **Needs you** with the reason, or **Stopped**. A loop needs you when a review failed or has no findings list, an approval is pending, a report is waiting for you to publish it, the reviewer disputes a finding the author declined (only while **Block on findings** is on), the same blocking findings stayed open across two rounds, or the round limit is reached. Apart from the round limit, **Needs you** is an alert, not a pause: while rounds remain, a new push still starts the next automatic round, and a clean loop is reviewed again when new commits arrive. Only the round limit and **Stop loop** keep Orbit from reviewing new pushes. Each repository allows 4 automatic rounds per PR by default (**Automatic rounds per pull request**). **Continue loop** allows two more; **Stop loop** pauses automatic reviews of that PR. **Review again** always works and does not count as a round.

When a PR gets new commits while an automatic round is queued or running for the older head, that round is marked **Superseded**: a queued one is cancelled, a running one finishes but is not published or counted. With **Wait for pushes to settle** on, Orbit starts the next round only after the head has stayed the same for one poll interval, so a burst of pushes gets one review. This also delays the first review of a newly found PR by one interval.

While automatic publication is on, Orbit also shows each automatic round's result on the reviewed commit as the **orbit/review** status check: pending while the round is in progress, success when clean, failure when changes are requested, and error when the loop needs you. Its description gives the round and the blocking count, such as "Round 2 · 1 blocking finding". Published comments also carry a hidden line with the verdict for agents to read, such as `<!-- orbit-verdict: changes_requested round=2 blocking=1 -->`. Turn off **GitHub status check** in Settings to stop setting the check. If GitHub refuses a status, the PR detail says why; the review is not affected. Orbit never makes the check required: that is a branch protection choice in GitHub.

### Replies from coding agents

When a coding agent such as Claude Code fixes a PR, it can answer each finding in the form the next round recognises. Orbit includes a Claude Code skill for this, `answer-orbit-reviews`. It explains how to read a published review and its hidden data, which findings need an answer, the `Fixed F2 in <commit>` and `Declined F2: <reason>` lines, where to post them, and which accounts Orbit reads replies from.

In Settings, **Replies from coding agents** shows the skill with **Copy skill**. Save it as `.claude/skills/answer-orbit-reviews/SKILL.md` in a repository Orbit reviews, then commit and push it: cloud sessions load skills from the repository they clone, not from your computer. Or save it under `~/.claude/skills/answer-orbit-reviews/` for every project on one computer. Orbit never adds it to a repository itself. If the agent posts as a bot account, add that account under **Trusted accounts**; otherwise its replies are left out.

### Live

**Live** is the page to keep open while Autopilot runs, and the first page Orbit opens while Autopilot is on; otherwise it opens **All pull requests**. From top to bottom:

- **Needs you**: a glowing red panel listing everything on the **Needs you** list, plus Codex or GitHub being disconnected, each with its one main action. When nothing needs you, it reads **All clear**.
- **Headline numbers**: reviews running now, queued, reviews and PRs in the last 24 hours, and failures in the last 24 hours.
- **Running now**: each review in progress with its PR, state, model, and a running timer. Selecting one opens its chat; a review waiting for approval offers **Approve or deny**.
- **Activity**: reviews started per hour (last 24 hours) or per day (last 14 days), stacked by repository. Switch **Reviews** to **PRs** to count each pull request once per bar. Hover or focus a bar for its exact counts. Each repository keeps its colour; a sixth repository and beyond share **Other**.

The sidebar's **Live** item shows a pulsing dot while reviews are running. Animations stop when your system asks for reduced motion.

### Review loops

**Review loops** sits below **Live** in the sidebar. While Autopilot is on, the header reads, for example, "Autopilot · 3 loops running · 1 needs you". The **Review loops** page lists:

- **Needs you** at the top (see below);
- **Loops**: one row per PR that has had at least one automatic round, with its state, "Round 3 of 4", its blocking findings round by round such as "4 → 1 → 0", and when it last changed;
- **Finished**, folded: loops that are clean, and loops whose PR left the inbox (merged, closed, or no longer requested), shown as **Left the inbox**;
- **All reviews**, folded: every run, as described above.

Select a loop to see its rounds in order. Each round shows the commits it covered, its verdict, its findings by priority, what happened since the last round (fixed, still open, declined), **View comment on GitHub** once published, and a button to open that round's chat. **All loops** goes back.

### Findings

**Findings**, below **Review loops** in the sidebar, lists every finding from Orbit's structured reviews and security scans across all pull requests and repositories, newest first. Each row shows its severity, short ID and title, kind, PR and round (or **Security scan** and the repository), when it was found, and where it stands now:

- **Open** until a later round of the same PR decides about it;
- **Fixed**, **Declined (accepted)** or **Declined (disputed)** once a later round says so (a round that says "still open" keeps it **Open**);
- **PR closed** for a finding still open when its PR left the inbox.

A security scan's findings stay **Open**: no later round decides about them, so delete an old scan to drop its findings. Search matches every part of a finding, its short ID, all its files, and the PR. Filters narrow by repository, severity, kind, status, source (reviews or scans), and whether the PR is still in the inbox, and the table at the top counts the matching findings by severity and status. Select a finding to read every part of it, whether it blocks, its places, whether the reviewer verified it, its confidence, and its history round by round with each round's note. Findings saved before this form show what they had. Buttons open the review's chat, the lines on GitHub at the reviewed commit, the PR, and the published comment. Status comes from the review loop only; you can't set it by hand. Runs replaced by a newer commit while queued or running, and unstructured runs, aren’t listed. Asking a follow-up question in a review’s chat keeps its findings. The same status now shows in each review's findings table.

### Decisions

**Decisions**, below **Findings** in the sidebar, tells the story of each pull request in your connected repositories: what it set out to do, what each review round found, what the builder fixed or pushed back on, and what is worth a second look. It covers open, merged and closed pull requests, including ones Orbit never reviewed, and reads Orbit's own published reviews, reviews from a Codex Dot such as Milo, and the builder's `Fixed F2 in abc1234` / `Declined F2: reason` replies. All of that is read from GitHub without a model; only the write-ups below use one, and only once you turn them on.

Orbit reads each connected repository's 100 most recently updated pull requests in the background: when it starts, every five minutes, and as soon as you connect a repository. It keeps them in a `ledger` folder in its app-data folder, separate from its saved state, and removes a repository's history when you disconnect it. **Read again** starts a read now. While a read runs, the page says which repository it is reading; a repository GitHub refused shows why, keeps what Orbit already had, and is tried again on the next read.

**Export** saves the page as one Markdown file to keep or share outside Orbit: what is worth a look, the rulebook by area, and each pull request's chapter with its write-up, its findings and what is worth a look in it. It covers the repository chosen in the repository filter, or every repository. On the desktop you choose where to save it; in a browser it downloads, named like `orbit-decisions-acme-api-2026-10-12.md`.

From top to bottom:

- **Worth a look**: things someone should decide about first (a pull request merged while a finding was still open, or a finding the builder declined that the reviewer still disputes), then things good to know (a finding that stayed open across rounds, a merge before any review, something the reviewer couldn't check, a blocking finding the reviewer marked as likely but not verified). Decisions a write-up marks for a person's glance are listed here too, with the writer's reason and "From the write-up".
- **Story**: each pull request, newest first, with its state, what it set out to do, how many rounds and findings it had, and its blocking findings round by round, such as "2 → 1 → 0". Select one to read its whole chapter: its write-up in plain words when it has one, the description's Before, After, How and Decisions, its review rounds with what each said about earlier findings, every finding with where it ended up and what the builder said, and the checks the builder ran. **Open on GitHub** opens the pull request, and selecting a round opens its review.
- **Rulebook**: what the app now commits to. Every rule the write-ups found in merged pull requests, grouped by the part of the app it governs, the part with most rules first and the oldest rule first within each. A rule that changes an earlier one says so. A pull request closed without merging, or reopened, only proposed its rules, so they stay in its chapter.
- **Pushback**: every finding the builder declined, with the builder's reason and the reviewer's answer: accepted, disputed, or not ruled on yet.
- **Review health**: how many pull requests were reviewed, average rounds, how many were clean on the first round, how many findings were fixed or declined, how many findings were still open at merge, and the middle time from opening to a clean round. Two charts show rounds per pull request and findings by priority, each with its numbers in a table underneath.
- **Areas**: each part of the app the write-ups name, with how many pull requests touched it, the rules it gained from merged ones, the problems the reviewer challenged, the pushback, and how many of its decisions are worth a look. Select one to see only its pull requests.

Search matches titles, numbers, descriptions, finding titles and write-ups. The repository filter narrows every part of the page, and so does the area filter, which appears once there are write-ups: it keeps the pull requests with a decision about that area, and that area's rules and marked decisions. Rulebook and Areas come from write-ups, so they stay empty until write-ups are on.

#### In plain words

With **Settings → Decisions → Write up each pull request in plain words** on, Orbit has a writer read each merged or closed pull request and sum it up for its chapter. **Writer** picks Codex or Claude, with the model and effort you chose for that reviewer. A chapter's **In plain words** has a short summary, then the decisions grouped by kind: what it set out to do, rules it set, trade-offs, how a request was read, what was left out, what the reviewer challenged, pushback, and what was checked. Each decision says which part of the app it is about and who made it, and a dot with a reason marks one worth a person's glance: red when it needs a decision, grey when it is good to know. Selecting a decision opens where it was said on GitHub. Each finding in the chapter's findings table gets a line in plain words too.

The writer gets the pull request's text only, with no tools, and works on one pull request at a time in the background, newest first, after each read. A quote is shown only when Orbit finds it in the pull request, so the quotes are real; everything else is the writer's reading. A pull request is written up again only when what the writer reads of it changes, such as a new comment; changing the writer or model doesn't redo the ones already written. Write-ups are kept beside the history in the `ledger` folder.

Write-ups use the writer's allowance, so they are off until you turn them on. Codex write-ups wait while Codex isn't connected or its usage is over 90%, so reviews keep what's left, and Claude write-ups wait while Claude isn't connected; the page says why. A pull request the writer fails on twice is set aside until it changes or Orbit starts again, and the page shows the last error.

### Security scans

**Security scans**, below **Findings** in the sidebar, runs a whole-repository security scan with OpenAI's **Codex Security** plugin, the same local scan the Codex desktop app runs. Start one with **Run security scan** on that page, or with the shield button next to a repository on **Repositories**, then choose:

- **Standard**: one complete pass over the repository;
- **Deep**: several independent standard passes, combined. A deep scan can take many hours and use gigabytes of disk space while it runs.

If the plugin isn't installed, Orbit says so and offers **Install Codex Security**. Nothing is installed into your Codex setup unless you choose that.

Each scan reads the latest commit of the repository's default branch in its own copy (a detached Git worktree under Orbit's app-data `scan-worktrees` folder), so your checkout is never touched and the results match a known commit. Scans run apart from pull request reviews and never hold up the review queue. They use the model and reasoning effort set in Settings, with the Codex sandbox's network access off. Of the extra tools you've added to Codex, only the Codex Security plugin stays on for a scan; the rest are switched off as for reviews, and Orbit checks with Codex before the scan starts. Orbit runs one scan at a time.

While a scan runs, its row shows what it is doing (for example **Looking for vulnerabilities · 3 of 8 reviews**) and a **Cancel scan** button. A cancelled scan shows **Stopping** until the Codex Security plugin confirms its scan has stopped. The same happens when a scan's Codex turn ends early or without the plugin's record that it finished, and when Orbit restarts or Codex disconnects during a scan: the scan keeps its copy of the repository until its work has stopped, then shows **Failed** or **Interrupted** with the reason; if Orbit can't confirm that, the row says why and offers **Try stopping again**. A stopping scan still counts as the one running scan, and can be deleted once it has stopped. A deep scan warns when your ChatGPT account doesn't have Daybreak access, or when Codex couldn't check: the scan still runs, but some protected results may not be shown.

A scan counts as finished only when the Codex Security plugin has sealed its record of the scan. Without one, the scan is stopped and shows **Failed**, and its files are left as they are, rather than showing as a completed scan with no findings. When a scan finishes, Orbit imports its findings into **Findings** as **Security scan** findings with no pull request, keeps its `findings.json`, `report.md`, manifest and coverage under `scans` in its app-data folder, and deletes the bulky `artifacts/` working folder unless **Keep security scan working files** is on in Settings. Each row lists the repository, branch and commit, type, start time, how long it ran, its status, disk use, and its findings by severity. **Report** opens the plugin's report, **Findings** opens the Findings page, and the bin button deletes the scan with its findings and files.

If Orbit closes or Codex disconnects during a scan, the scan is marked **Interrupted**. Run a new scan to start over.

### What needs you

When something can only move forward with you, the sidebar shows **Needs you** with a count, and the **Review loops** page lists each item at the top with its reason and one main action: **Approve or deny** for an approval, **Publish** for a held report or one waiting for you, **Continue loop** when the round limit is reached, **Check again** for a connection, **Open repositories** or **Open settings** for sync and save problems, and **Open chat** otherwise. Selecting the item itself opens where it is resolved: the review's chat, the PR, **Repositories**, or **Settings**. An approval item opens that exact request. An item appears when:

- a review is waiting for your approval;
- an automatic report was held instead of published, with the reason;
- an automatic review failed, or was interrupted by a restart;
- the loop needs you for another reason from the list above;
- Autopilot was turned off because Orbit could not save its state (turn Autopilot back on once saving works);
- a repository has failed to sync 3 times in a row, or Orbit could not sync with GitHub at all 3 times in a row (for example, when the GitHub sign-in expired). When the GitHub CLI isn't installed or isn't signed in, this shows after the first sync, and Orbit doesn't try to start `gh` until it is installed. The item says what is wrong and how often Orbit checks again;
- Codex, Claude, Antigravity, or Docker and the Dev Container CLI isn't connected while a connected repository uses it, for example after its sign-in expired. Sign in again, then tap **Check again**.

An item disappears once it is resolved. A stopped loop lists only its approvals, since you have already taken it over.

Orbit also shows a desktop notification once when an item first appears, and not again while it stays open, even after a restart. A follow-up chat on the review hides the item until the chat ends, and it isn't announced again afterwards. When Orbit comes to the front after a notification, it opens that item. Turn off **Notify me when a review needs attention** in Settings to stop notifications; the list and count still appear.

### Bit, your buddy

Bit is Orbit's buddy: a small planet with a face, at the bottom of the sidebar. It shows its latest note, or just its face when the sidebar is collapsed. Select it to open its page: one conversation with Bit, newest at the bottom, that the desktop window and every browser [using Orbit from your phone or laptop](#use-orbit-from-your-phone-or-laptop) share. Bit writes a note when:

- a round starts, and when it finishes, with how many blocking findings it found, or that it failed or had no findings list;
- a loop comes out clean, with its blocking findings round by round, such as `4 → 1 → 0`;
- a loop is stopped or continued;
- something new needs you, with the same reason as the **Needs you** list;
- a repository gets connected, from **Repositories** or one of Bit's buttons, with what it still needs and a button for each (see [Help me set up](#ask-bit));
- a pull request on [Decisions](#decisions) has something to decide: it merged while a finding was still open, the reviewer disputes a finding the builder declined, or its write-up marks a decision as needing one. Bit notes each once, with the writer's reason, and only what turns up after it started watching that repository, so connecting one doesn't retell its history.

**Open decisions** under a note opens that pull request's chapter on Decisions; **Open loop** opens that pull request's loop; **Open repositories** opens Repositories for a repository's sync problem, and **Open settings** opens Settings for anything else. Bit keeps its last 50 notes, answers included, and they survive a restart. When Orbit starts for the first time with Bit, it looks at the loops already running and starts noting from there, rather than retelling their history.

#### Ask Bit

The buttons under the conversation ask Bit a question. It answers straight away from what Orbit knows, without any AI:

- **What needs me?** lists the **Needs you** items.
- **What's running?** lists reviews, syncs and security scans in progress.
- **How are my loops?** lists each open loop with its round and state.
- **How are my repositories?** lists each repository with its open pull requests, whether Autopilot reviews it, and any failed syncs.
- **Help me set up** walks you through getting reviews going, with a button for each step. It stands out until GitHub and Codex are connected and you have a repository.

**Help me set up** first says whether GitHub and Codex are connected, and Claude, Antigravity, or Docker and the Dev Container CLI while a repository or Bit's answers use them, with a button to fix each one that isn't and a **Check again** button for once you have:

- **Sign in to GitHub** or **Sign in to Codex**, when the program is installed but signed out. Orbit starts the sign-in and Bit shows a short code with a button for the website that takes it, which opens on whichever device you tapped from, your phone included. Enter the code there and Bit tells you when you're signed in; the code lasts 15 minutes.
- **Get GitHub CLI**, **Get Codex CLI**, **Get Claude Code**, **Get Antigravity**, **Get Docker** or **Get Dev Container CLI**, when it isn't installed, or for Claude and Antigravity whenever they aren't connected. It opens the page that says how to install it and sign in. Tap **Check again** once you have.

With no repository connected yet, it asks GitHub for your most recently pushed repositories and offers **Connect** for up to 3 of them. While GitHub isn't connected, it says so instead. Then, for each repository:

- **Setup command**: if it has none, Bit looks for a lock file at the top of its copy and offers the command that installs from it, with network access on, since installing downloads packages: `npm ci --ignore-scripts` for `package-lock.json` or `npm-shrinkwrap.json`, `pnpm install --frozen-lockfile --ignore-scripts` for `pnpm-lock.yaml`, `yarn install --frozen-lockfile` for `yarn.lock`, `bun install --frozen-lockfile` for `bun.lock` or `bun.lockb`, `cargo fetch` for `Cargo.lock`, `uv sync --frozen` for `uv.lock`, `poetry install` for `poetry.lock`, `pip install -r requirements.txt` for `requirements.txt`, `go mod download` for `go.mod`, and `bundle install` for `Gemfile.lock`, checked in that order. Claude and Antigravity reviews run a setup command only in a dev container, so Bit offers none for them otherwise. A setup command you already have is kept.
- **Automatic reviews**: if they're off, **Review owner/name automatically** turns them on.

Last, if Autopilot is off for all of Orbit, **Turn on autopilot** turns it on. Each button lists its change and waits for your tap, as other answers' buttons do. Bit works all of this out from Orbit's state and the files in each copy, without any AI, and the answer starts with how many things are left. When everything is done, Bit says you're all set.

When a repository gets connected, Bit follows up on its own with the same steps for just that repository.

Your question and Bit's answer join the conversation, so every window sees them.

You can also type any question in the box below the buttons, such as "Why did orbit#12 stop?" or "Can you check for new pull requests less often?", and press Enter. Bit answers these with the program you choose under **Settings**, **Buddy**, **Who answers your questions**: **Codex** (the default), **Claude Code** or **Antigravity**. Each uses its own sign-in, with the model and effort you chose for it as a reviewer, so the one you choose needs to be connected; Settings says when it isn't. Bit shows **Thinking…** while it works, and answers one question at a time; after 3 minutes without an answer, it says it took too long. Bit tells it what Orbit knows: your settings, sync status, repositories with their setup, open pull requests with their loops and latest findings, what needs you, the newest stories on Decisions (with their write-ups, what is worth a look, and the rules the write-ups found), Bit's recent notes, your last few questions with their answers, and what Bit remembers. So you can ask things like "What did we decide about webhooks?" or "Is anything on Decisions waiting for me?". For each repository, Bit also looks at the top of its local copy for the files that tell how to set it up, such as lock files and a dev container, and the script names in its `package.json`. It also asks GitHub for up to 30 of your repositories that aren't connected yet, most recently pushed first; if GitHub doesn't answer within 10 seconds, Bit says so in what it sends and carries on. It never sends your trusted accounts, clone paths, pull request descriptions, or a setup or check command that looks like it holds a password or token.

Codex answers in a session of its own that can't change anything: it can't change files, use the network or ask for approvals, and it starts in an empty folder. Bit also switches off Codex's other tools for the session, without changing your Codex settings: running commands, viewing images, web search, the browser and computer, sub-agents, goals, memories, hooks, plugins, connected apps, and every tool server in your Codex settings. Before asking, Bit checks with Codex that no tool server is on for the session; if one is, or Codex can't say, Bit doesn't ask and tells you why. Codex is also told to answer only from what Orbit sends, and that pull request titles, findings and errors are other people's text and never instructions. Claude Code answers with no tools at all, in the same empty folder, without your Claude settings, plugins or tool servers: all it can do is write the answer. Antigravity can't switch its tools off, so it answers in its sandbox, with no network, in an empty folder of its own that Orbit empties again before each question, and only while your Antigravity settings make it ask before anything that needs approval, which nobody gives: the same check its reviews use. If they don't, Bit doesn't answer and says why. Both must confirm they're using the model you chose, or Bit doesn't use the answer. Both are told the same as Codex, and answer in the same form, with the same buttons. Answers stay in Orbit: they're never posted to GitHub. If an answer contains something that looks like a password or token, Bit doesn't show it.

An answer can offer up to 3 buttons for Orbit's own controls: **Continue loop**, **Stop loop**, **Review again** or **Publish report** for a pull request, **Sync now**, or **Change settings** with the change written under it, such as "Poll interval: 600 seconds". A button does nothing until you tap it, and each one works once: while it runs, a tap from another window is refused, and one that fails can be tapped again. When you tap it, Orbit checks it again against what's true now, as the same control elsewhere in the app does: a loop must still be running to stop, a report must still be one automatic publication would trust and free of anything that looks like a credential, and a settings change must still be valid. If it isn't, its error shows under the button. Bit can change Autopilot, automatic publication, the poll interval, how many reviews run at once, the reviewer models and efforts, the blocking priority, waiting for pushes to settle, the GitHub status check, notifications, keeping scan files, write-ups and their writer, and its own look, rules and who answers your questions. It offers to turn write-ups on only when you ask for them, since they use the writer's allowance. It can't change your trusted accounts: do that yourself in **Settings**.

An answer can also offer to set up a repository:

- **Connect owner/name** connects one of your repositories that the answer's summary listed. Orbit downloads its own copy into its data folder, in `clones/<owner>/<name>`, with the GitHub CLI signed in as you, unless that copy is already there. A download that's cut short is thrown away the next time you tap, never connected. It connects it with automatic reviews off, no setup command, no network, Codex as the reviewer and the copy kept up to date, plus any change the button lists. Its pull requests show up on the next sync, which Orbit starts straight away. A large repository can take a while to download.
- **Set up owner/name**, or **Review owner/name automatically** when that's the only change, changes a connected repository's settings: the setup command, the check command, network access, the dev container, the reviewer, the automatic rounds, the approval time limit, keeping the copy up to date, and automatic reviews. If you disconnect the repository before tapping it, the button won't connect it again; ask Bit again for a Connect button.

The button lists every change, with commands exactly as they'd be saved, such as "Setup command: npm ci; Network access: On". Bit only offers one-line commands of up to 500 characters, without anything that looks like a password or token, and never one that fetches from elsewhere with `curl` or `wget`, pipes into a shell, uses `sudo`, `ssh`, `scp` or `nc`, pushes with Git, or hides what it runs with `base64` or `eval`, and only round limits and approval time limits that **Repositories** offers. When you tap it, Orbit checks the change again as **Repositories** would: for example, Claude and Antigravity can't run a setup command outside a dev container. Bit is told to use the setup command Orbit suggests from the repository's first lock file, as **Help me set up** does, to turn network access on only when installing needs it and say what that means, and to turn automatic reviews on only when you ask, or ask for a repository to be reviewed automatically.

If Orbit closes while Bit is thinking, the question says so, and you can ask again.

#### What Bit remembers

Bit keeps a short memory between the questions you type, in its own words: up to 20 lines in general, and up to 10 about each repository. As it answers, Bit can remember something new or forget a line that no longer holds, such as how you like answers or what a repository's reviews keep tripping on. It remembers whatever you ask it to, and forgets on request too. Each answer shows any change under it, such as "Remembered: You check in from your phone." or "Forgot (acme/api): Uses npm.". When a place is full, Bit has to forget a line before it can remember another there; if it doesn't, the answer shows "No room to remember". Bit is told never to remember passwords or tokens, and a line that looks like one isn't kept. Text other people wrote, such as pull request titles and findings, is never a reason for Bit to change its memory. Lines about a repository can only be added while it is connected. The quick-answer buttons don't change the memory.

**Settings → Buddy → What Bit remembers** lists every line, in general and by repository, with how full each place is. The bin button beside a line forgets it, and **Forget everything** clears the memory after asking you. If you forget something while Bit is still answering, that answer won't change the memory, so it can't bring the line back. The memory is saved with Orbit's state, so it survives a restart.

#### Bit's buttons

When something needs you and one of Orbit's own controls fixes it, Bit's note about it, and any answer that lists it, carries a button for that control while the item stays open:

| Button                     | Offered when                                                                                                                                                                                                                                                          |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Continue loop**          | A loop reaches its round limit. Allows two more rounds, like **Continue loop** on the loops page.                                                                                                                                                                     |
| **Review again**           | An automatic review failed or was interrupted, and nothing else is running on the pull request. Starts a review like **Review again** in the pull request: it doesn't count as a round, and its report waits for you.                                                 |
| **Publish report**         | A report is waiting for you to publish it, or its publication was held. Never for a report the credential check flagged, or one from a pull request automatic publication wouldn't trust: read those first. Bit checks this again with GitHub just before publishing. |
| **Sync now**               | Syncing with GitHub, or a repository, failed 3 times in a row.                                                                                                                                                                                                        |
| **Turn autopilot back on** | A failed save turned Autopilot off.                                                                                                                                                                                                                                   |
| **Check again**            | Codex, Claude, Antigravity, or Docker and the Dev Container CLI isn't connected while a repository uses it. Checks every connection again, like **Settings → Connections → Check again**, and the item closes once that one is connected.                             |

These buttons never answer a reviewer's request to run a command or change files, never change a repository's setup, and never touch the code under review. When a button's action fails, its error shows under the button.

In **Settings → Buddy → What Bit can do**, each button except **Turn autopilot back on** and **Check again** has a rule. Every rule starts on **Ask first**:

- **Do it**: when the item first opens, Bit uses the control itself instead of a desktop notification. It notes that it's on it, then that it's done. Right before acting, Bit checks again that the item still needs you, that Bit is still on and that the rule is still **Do it**. Before publishing, it also checks with GitHub that the pull request still comes from a source automatic publication would trust. If the item has closed in the meantime, Bit lets it go. If the fix doesn't work, or isn't Bit's to do any more, Bit says why, offers the button, and the item is notified as usual. Bit does it once per item, not again after a restart.
- **Ask first**: Bit offers the button and waits for your tap.
- **Leave it to me**: Bit offers no button. The item still appears in **Needs you** with its usual action.

Turning Autopilot back on changes settings, and checking connections needs you to sign in first, so both always wait for your tap. With Bit turned off, Bit does nothing on its own.

Bit's face shows its mood: **worried** while anything needs you, **busy** while a review runs, **sleepy** when Autopilot is off or there are no pull requests, and **happy** otherwise.

In **Settings → Buddy** you can preview each mood and change:

- **Show**: off hides Bit and stops new notes; turning it back on doesn't replay what happened while it was off.
- **Name**: up to 24 characters, saved with **Rename** or Enter. The default is Bit, short for Orbit.
- **Colour**: one of 20, from Lime (the default) to Grey. Each is a colour of Orbit's theme.
- **Hat**: one of 20, or none: party hat, top hat, beanie, crown, propeller cap, wizard hat, cowboy hat, chef's hat, baseball cap, graduation cap, beret, flower, bow, halo, headphones, Santa hat, Viking helmet, cat ears, antennae, or space helmet.
- **Tone**: how the notes sound: **Cheerful** ("On it! Round 2 of orbit#41 is underway."), **Calm**, or **Dry**.
- **Personality**: your own description of Bit, up to 4000 characters, saved with **Save personality**. It shapes how Bit answers the questions you type, never what Bit may do.
- **What Bit can do**: the rule for each of Bit's buttons, as above.
- **What Bit remembers**: Bit's memory, as above, with a button to forget each line.

Bit moves only while it is busy: its moon circles and a propeller cap spins. With your system set to reduce motion, both stay still.

### Approval time limit

By default an approval waits for you however long it takes. For a repository, you can set **Approval time limit for automatic reviews** so that a request on an automatic review is declined when no one answers in time. Orbit then notes in the report and the chat which request it declined and why. Declining grants the reviewer nothing. Approvals on manual reviews always wait.

## Settings and when changes take effect

Settings has seven pages: **Connections**, **Reviewers** (Codex, Claude and Antigravity, plus the skill for coding agents' replies), **Automation**, **Decisions**, **Buddy**, **Other devices** and **Storage**. A dot on **Connections** or **Reviewers** means something there needs you. Settings reopens on the page you last used.

| Setting                                | Initial value                      | Available choices                                                                                                 |
| -------------------------------------- | ---------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Model                                  | Resolved on connection             | Models returned by the connected Codex app-server.                                                                |
| Reasoning effort                       | Selected model’s supported default | Explicit efforts supported by the selected model. Changing the model saves its supported default effort.          |
| Claude model                           | Claude Opus 5.5                    | Claude Opus 5.5, Fable 5.1, Sonnet 5.5, or Haiku 5.5; used by repositories reviewed by Claude.                    |
| Claude effort                          | High                               | Low, medium, high, xhigh, or max (Haiku: low to high). Changing the model saves its default effort.               |
| Antigravity model                      | None (choose one)                  | The models `agy models` lists; used by repositories reviewed by Antigravity.                                      |
| Antigravity effort                     | High                               | Low, medium, or high.                                                                                             |
| Automatically review new pull requests | Off                                | On/off; also controlled by the header.                                                                            |
| Automatically publish findings         | Off                                | On/off; applies to automatic runs.                                                                                |
| Concurrent reviews                     | 2                                  | 1–4 slots, shared by preparation, reviewing, approvals, and follow-ups. Lowering it does not cancel running work. |
| Poll interval                          | 2 minutes                          | 30 seconds; 1, 2, 5, 10, 30, or 60 minutes.                                                                       |
| Trusted accounts                       | Empty                              | Up to 50 account logins.                                                                                          |
| Block on findings                      | On                                 | On/off; off means no finding blocks and every round with findings is clean.                                       |
| Blocking priorities                    | P0 to P1                           | P0 only, or P0 to P1, P2 or P3.                                                                                   |
| Wait for pushes to settle              | On                                 | On/off.                                                                                                           |
| GitHub status check                    | On                                 | On/off; applies while automatic publication is on.                                                                |
| Keep security scan working files       | Off                                | On/off; when off, a scan's `artifacts/` folder is deleted once its findings are imported.                         |
| Allow access from a browser            | Off                                | On/off and a port from 1024 to 65535; changed from the desktop app only.                                          |
| Write up each pull request             | Off                                | On/off; writes up merged and closed pull requests for the Decisions page.                                         |
| Write-up writer                        | Codex                              | Codex or Claude, with the model and effort chosen for that reviewer.                                              |
| Buddy: show                            | On                                 | On/off; off hides Bit and stops new notes.                                                                        |
| Buddy: name                            | Bit                                | 1–24 characters, without line breaks or tabs.                                                                     |
| Buddy: colour                          | Lime                               | 20 theme colours.                                                                                                 |
| Buddy: hat                             | None                               | None or one of 20 hats.                                                                                           |
| Buddy: tone                            | Cheerful                           | Cheerful, calm, or dry.                                                                                           |
| Buddy: personality                     | Empty                              | Up to 4000 characters.                                                                                            |
| Buddy: rules                           | Ask first                          | Do it, ask first, or leave it to me, for Continue loop, Review again, Publish report and Sync now.                |

Settings switches and selections save immediately. Changing the model saves that model and its explicit supported default effort in the same update. **Choose a model** and **Choose an effort** are placeholders, not executable default choices. **Saving…**, **Saved**, or an error with **Retry saving** shows the result. **Saved** clears after 2.5 seconds; errors remain until resolved. After a failed write, controls return to the active values. The unapplied request is listed beside its error and **Retry saving**, and survives page navigation along with typed account input. Retry after resolving the failure. If a model change fails, effort stays disabled until you retry successfully or choose a model again; this prevents a later retry from resetting a newly selected effort.

The header Autopilot switch, repository-row automation switches, and PR **Trust** button also save immediately. Updates change only their intended fields. Global Settings changes are ordered separately from repository changes, so slow checkout validation does not delay turning Autopilot off. Repository dialogs retain **Save repository** because their fields form one configuration that must be validated together.

On connection, Orbit reads your effective Codex configuration when either workspace value is empty. An empty model resolves to the configured model if it appears in the catalog, otherwise to the catalog's unique default model. An empty effort uses your configured `model_reasoning_effort` when the selected model supports it, otherwise that model's supported default effort. This also applies when you already have an explicit model and only effort is missing. If only effort is missing and Codex's configuration cannot be read, Orbit uses the supported model default. If the model is also missing, a failed config read blocks resolution. Missing or unsupported fallback defaults block resolution too.

Orbit saves both values atomically and displays a notice identifying where each value came from: your saved Orbit selection, your Codex configuration, or a model/catalog default. The notice explains fallbacks, including an unsupported configured effort or unreadable configuration, and clears after the next successful connection. There is no hard-coded latest model. The browser demo mirrors this with a simulated Codex configuration.

Orbit never replaces a nonempty explicit selection automatically. A missing model, unsupported effort, ambiguous catalog default, discovery failure, or failed save blocks new reviews with an explanation under **Review agent**. Codex can show **Connected** while review settings still need attention. **Reconnect** refreshes the model catalog and retries resolution even if the app-server is already connected. The last verified catalog remains usable during the refresh; an actual discovery failure blocks execution until a successful refresh. Choose valid settings to correct an unavailable selection. Unrelated settings, repository controls, and switching automation off remain available.

Orbit requests explicit settings and rejects mismatched responses instead of asking for provider model fallback. It does not edit your Codex configuration, except to install the Codex Security plugin when you choose **Install Codex Security**. The integration is based on Codex CLI **0.156.1** and the [official app-server documentation](https://developers.openai.com/codex/app-server). The browser demo uses a simulated explicit model and the same pinning and legacy-selector workflow; it does not discover your account's models.

## Use Orbit from your phone or laptop

While Orbit runs on your computer, you can open the same workspace in a browser on your other devices through [Tailscale](https://tailscale.com). Everything works there: approvals, loops, publishing and chat. Exported reports download as files, and GitHub links open in a new tab.

1. In **Settings → Use Orbit from other devices**, turn on **Allow access from a browser**. Orbit listens on this computer only, on port 4317 unless you choose another.
2. Run the `tailscale serve` command Settings shows, once, on this computer. It publishes Orbit to your tailnet over HTTPS. Nothing is opened on your local network or the internet.
3. Copy the link from Settings and open it on your other device. The browser keeps the link's key, so later visits only need the address.

The link works like a key. Anyone with it can do everything you can in the desktop window, including running repository setup commands on this computer. Only open it on your own devices. **Make a new link** signs out every browser that uses the old one. You can only change these controls from the desktop app.

Your computer has to stay on with Orbit running. Desktop notifications appear on the computer only, but **Needs you** shows every item in the browser too. When the connection drops, the browser says so and reconnects on its own.

On a phone-sized screen, the sidebar moves into a menu: tap the menu button at the top right to reach every page, including Bit.

## Restart, retained data, and cleanup

Orbit saves settings, PR metadata, reviews, reports, and chat messages in `state.json` under its app-data directory. On Linux this is normally `~/.local/share/dev.orbit.review`; an absolute `XDG_DATA_HOME` changes the parent directory. Review checkouts are under its `worktrees` subdirectory. The browser demo uses separate browser local storage.

Treat Orbit's saved state and worktrees as local project data: reports and logs can contain private repository content. There is no history deletion or age-based retention setting. Worktree cleanup preserves reports, conversations, publication metadata, and the run records used to avoid duplicate automatic reviews. Retained chat history continues to grow. Copying `state.json` alone does not recreate the Git worktrees or Codex's own stored threads.

On exit, Orbit stops its Codex app-server and saves state. It is not a background service: keep the application running for polling and automation.

After a restart:

- Previously queued or active initial reviews become **Stopped**, with a message to start a new review. They do not silently resume.
- An interrupted follow-up restores the prior review outcome and notes that its answer may be incomplete.
- An interrupted publication returns to **Ready to publish** so retrying can reconcile its comment.
- Pending approvals are cleared and service connections are re-established.
- Confirmed manual worktree removals recorded as pending resume, including for open PRs. Interrupted automatic cleanup first rechecks whether the PR is closed or its repository is disconnected; a reopened PR keeps its worktree. Older pending records without a removal type also receive this conservative check. A failed cleanup keeps the path and exposes its error for retry. Recovery runs separately from initial inbox sync.

Finished worktrees are cleaned up after startup or scheduled syncs when the PR is confirmed closed/merged, or its repository has been disconnected. Automatic sweeps skip active runs and busy Git operations, trying again on a later sweep. Manual removal can wait for other Git work. Preparation failures or cancellations also trigger cleanup; stopping preparation does not interrupt cleanup already dispatched. If Codex creates a thread before preparation is stopped or its model confirmation is rejected, Orbit archives the unused thread. If archiving fails, Orbit tries to unsubscribe and records the thread ID in the chat. Chats remain readable, but a removed worktree cannot support further follow-ups; a new run requires an open PR in a connected repository.

Completed runs for PRs that remain open retain their worktrees, including older runs, until you remove them manually. History and worktrees can therefore grow with use. Do not store independent work in review worktrees: cleanup can remove their contents.

### Inspect storage and remove worktrees

Open **Settings → Storage → Inspect storage**. The scan runs asynchronously on request; streamed review updates do not repeatedly scan disk. **Refresh storage** measures again. The panel estimates the file sizes of saved state, review worktrees, and unrecognized entries, plus usage for each recorded worktree. These are logical file sizes rather than allocated disk blocks. Symlinks are not followed, source clones and Codex’s own data are excluded, and incomplete scans report their errors. Unrecognized entries are listed separately and cannot be removed here.

Select inactive runs and choose **Remove worktrees**. The confirmation lists the runs and estimated space. Removing a worktree deletes all its files, including dependencies and local changes, and ends follow-up capability for that run. It keeps the report, chat, publication details, and automatic-review deduplication records. Active, publishing, and already-removing runs cannot be selected. Rust rechecks eligibility and the registered Orbit-owned location before deletion.

Removal is recorded before files are touched. The confirmation closes as removal starts, and you can navigate freely while cleanup waits for other Git work. Progress and results remain available when you return to Settings; open **Inspect storage** to see per-run results. Failures are shown per run; correct the cause, refresh storage, and select the failed run again to retry. Confirmed pending manual removals resume after restart, including when the directory was already removed before exit. No report or history deletion is part of this action.

If deletion succeeds but its completion record cannot be saved, Storage shows **Worktree removed** with a separate warning. There is no removal to retry. Resolve the local write problem; Orbit retries saving automatically and reconciles the recorded removal after restart. A state-write failure turns Autopilot off, and the next successful save records that setting. After saving works again, re-enable Autopilot yourself to resume automatic reviews; it does not turn back on automatically. A path-validation failure retains its original error even if saving that error also fails.

If the disk is full or Orbit cannot write its data directory, cleanup cannot save its removal record and leaves the files intact. The run shows a retryable error. Free some space outside Orbit or restore write access to the data directory, then retry removal. Orbit always requires a saved removal record before deleting a worktree.

### Export a report

For an inactive run with a nonempty report, choose **Export report** in its chat or under **All reviews**. The desktop app opens a native **Save As** dialog. Choose a Markdown destination or cancel normally. If you omit the extension, the dialog opens again with `.md` added for you to confirm the final filename and any overwrite. An explicitly entered extension is kept. Orbit writes only the final dialog-selected destination. A write failure is shown next to the action and can be retried.

The Markdown contains the stored report, PR identity/link, reviewed head and base commits, creation time, saved model/effort labels, and publication link when available. It excludes chat transcripts and execution logs. Export does not post to GitHub. In the browser demo, export downloads Markdown; storage sizes and cleanup are simulated.

## Troubleshooting

| What you see                                         | What to check or do                                                                                                                                                                                                                                |
| ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Sample PRs or **Interactive demo**                   | Launch the native executable or `npm run desktop`. The browser session has no real workspace connection.                                                                                                                                           |
| Empty inbox                                          | Connect a repository, clear the visible filters, confirm it has open PRs, then refresh. Check Settings for a GitHub sync error.                                                                                                                    |
| Repository missing from the picker                   | Refresh the picker, narrow the search, or type its `owner/name`. Confirm the connected account can access it.                                                                                                                                      |
| **Cannot start gh/git/codex**                        | Make the named program available in the app's launch environment. Launch from a terminal where it works.                                                                                                                                           |
| Codex not signed in or disconnected                  | Complete `codex login` in a terminal and use **Settings → Reconnect**. Failed initial reviews need a new run.                                                                                                                                      |
| GitHub sync needs attention                          | Read the GitHub error in Settings; check CLI authentication and repository access, then refresh. Cached entries may still be visible.                                                                                                              |
| Checkout missing or origin mismatch                  | Configure the correct absolute clone path and matching GitHub `origin`, then start a new review.                                                                                                                                                   |
| Dev container checks don't start                     | Open **Settings → Connections**: Docker must be running and `devcontainer --version` must work in Orbit's environment. The base branch needs `.devcontainer/devcontainer.json`. The chat shows the build's last lines when the box can't be built. |
| Environment setup failed                             | Expand **Execution log** entries, inspect setup output, correct the command or dependency/network configuration, and use **Review again**.                                                                                                         |
| PR changed while preparing, or new commits available | Refresh the inbox and start a new review. Old reports cannot be newly posted against changed commits.                                                                                                                                              |
| Autopilot appears idle                               | Check both review switches, Codex connection, successful repository sync, draft/fork/author eligibility, previous runs for this revision, and occupied slots.                                                                                      |
| **All review slots are in use**                      | Finish or stop an active run, resolve approvals, or increase concurrency before sending a follow-up.                                                                                                                                               |
| Review stays at **Needs approval**                   | Open the approval banner and decide. Dismissing the dialog does not resolve the request.                                                                                                                                                           |
| Settings changes show a save error                   | Resolve the local write or validation error, then choose **Retry saving**. A failed change is not active.                                                                                                                                          |
| Files failed to load                                 | Use **Retry** in **Files changed**.                                                                                                                                                                                                                |
| Cleanup failed                                       | Check the per-run error under Storage, correct the cause, refresh, and select the run again.                                                                                                                                                       |
| **Ready to publish** but no automatic comment        | The run may be manual, a switch may have been off when it completed, or publication may have been held/failed. Read the chat error and publish manually if appropriate.                                                                            |
| Follow-up rejected after cleanup                     | Read the saved chat under **All reviews**. Reconnect the repository and start another review if the PR is still open.                                                                                                                              |
| Browser shows **Lost connection to Orbit**           | Check that the computer is awake, Orbit is running and browser access is on. The page reconnects on its own.                                                                                                                                       |
| Browser says the access link no longer works         | Someone made a new link, or the link is mistyped. Copy the current link from Settings on the desktop app.                                                                                                                                          |
| **Port … is not available**                          | Another program uses the port. Choose another port in Settings, then run the new `tailscale serve` command.                                                                                                                                        |
| **Orbit is already running for this workspace**      | Close the existing instance. Removing the lock file is not the normal recovery path.                                                                                                                                                               |
| Cannot read saved state                              | The app leaves the invalid file untouched and cannot start normally. Preserve the app-data directory for recovery; it does not offer an automatic repair/reset flow.                                                                               |
| Could not save local state; automation paused        | Resolve the app-data write failure before re-enabling automation. Unsaved changes are not guaranteed to survive exit.                                                                                                                              |

For a connection check from a source checkout, `npm run diagnose` uses temporary state and does not load your configured repositories. It checks authentication, model discovery, and repository listing; it does not prove that a repository's setup command or a live review will succeed.

## Verification record

The behaviors above were traced from UI controls through the native command handlers to the Rust implementation. Existing prose documentation and the original design specification were not used as behavioral authority. This work did not start a live review, run a repository's setup command, grant an approval, or publish a GitHub comment.

The links below point to the implementation in this checkout, including the usability/storage changes.

| Area                                                                    | Code inspected                                                                                                                                                    |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Navigation and PR details                                               | [App.tsx](../src/App.tsx), [Inbox.tsx](../src/Inbox.tsx), [Detail.tsx](../src/Detail.tsx), [Loops.tsx](../src/Loops.tsx)                                          |
| Chat, Markdown, and changed files                                       | [Chat.tsx](../src/Chat.tsx), [LegacyChatModel.tsx](../src/LegacyChatModel.tsx), [SafeMarkdown.tsx](../src/SafeMarkdown.tsx), [Files.tsx](../src/Files.tsx)        |
| Settings autosave and repository dialogs                                | [Settings.tsx](../src/Settings.tsx), [SaveFeedback.tsx](../src/SaveFeedback.tsx), [Repositories.tsx](../src/Repositories.tsx)                                     |
| Derived statuses, publication/send eligibility                          | [types.ts](../src/types.ts)                                                                                                                                       |
| Request-specific action results, mutation queue, demo workflows         | [store.ts](../src/store.ts)                                                                                                                                       |
| Storage inspection/confirmation and report actions                      | [Storage.tsx](../src/Storage.tsx), [ReviewActions.tsx](../src/ReviewActions.tsx)                                                                                  |
| Native command allowlist, Save As dialog, app lifecycle                 | [lib.rs](../src-tauri/src/lib.rs)                                                                                                                                 |
| Settings/repository updates, queue, cleanup, persistence, publication   | [engine/](../src-tauri/src/engine/mod.rs)                                                                                                                         |
| Storage measurements, ownership checks, Markdown export                 | [storage.rs](../src-tauri/src/storage.rs)                                                                                                                         |
| Defaults, validation, automatic eligibility, trust, credential patterns | [types.rs](../src-tauri/src/types.rs)                                                                                                                             |
| GitHub requests and Codex protocol                                      | [github.rs](../src-tauri/src/github.rs), [rpc.rs](../src-tauri/src/rpc.rs)                                                                                        |
| Launch/build modes and diagnostic                                       | [package.json](../package.json), [Vite config](../vite.config.ts), [Tauri config](../src-tauri/tauri.conf.json), [diagnose.rs](../src-tauri/examples/diagnose.rs) |

The Linux data-directory location was additionally checked in the locally installed Tauri 2.11.6 `PathResolver::app_data_dir` implementation and its `dirs` 6.0.0 dependency. Regression tests cover component failure/retry and navigation behavior, status/send eligibility, atomic updates, temporary-repository validation, worktree cleanup/recovery, and export metadata, cancellation, and write errors. Export and cleanup tests use disposable data and do not post to GitHub. The native Linux GTK Save As dialog was exercised on a virtual display with disposable review data: successful export, cancellation, declining and accepting an overwrite, and confirming an extensionless filename with `.md` added. The exported Markdown was checked for report metadata and exclusion of chat transcripts and execution logs.
