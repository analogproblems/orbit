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

When a coding agent such as Claude Code fixes a PR, it can answer each finding in the form the next round recognises. Orbit includes a Claude Code skill for this, `answer-orbit-reviews`. It explains how to read a published review and its hidden data, which findings need an answer, the `Fixed F2 in <commit>` and `Declined F2: <reason>` lines, the answers to Orbit's CI comments and issues (`Taking I2`, `Fixed I2 in <commit>`, `Declined I2: <reason>`, `Need a person on I2: <question>`), where to post them, which accounts Orbit reads replies from, and to merge only when Orbit's own comment, posted by Orbit's account, says clear to merge at the agent's latest commit and base (see [Clear to merge](#clear-to-merge)).

In Settings, **Replies from coding agents** shows the skill with **Copy skill**. Save it as `.claude/skills/answer-orbit-reviews/SKILL.md` in a repository Orbit reviews, then commit and push it: cloud sessions load skills from the repository they clone, not from your computer. Or save it under `~/.claude/skills/answer-orbit-reviews/` for every project on one computer. Orbit never adds it to a repository itself. If the agent posts as a bot account, add that account under **Trusted accounts**; otherwise its replies are left out.

### Live

**Live** is the page to keep open while Autopilot runs, and the first page Orbit opens while Autopilot is on; otherwise it opens **All pull requests**. From top to bottom:

- **Needs you**: a glowing red panel listing everything on the **Needs you** list, plus Codex or GitHub being disconnected, each with its one main action. When nothing needs you, it reads **All clear**.
- **Headline numbers**: reviews running now, queued, reviews and PRs in the last 24 hours, and failures in the last 24 hours.
- **Running now**: each review in progress with its PR, state, model, and a running timer. Selecting one opens its chat; a review waiting for approval offers **Approve or deny**.
- **Activity**: reviews started per hour (last 24 hours) or per day (last 14 days), stacked by repository. Switch **Reviews** to **PRs** to count each pull request once per bar. Hover or focus a bar for its exact counts. Each repository keeps its colour; a sixth repository and beyond share **Other**.

The sidebar's **Live** item shows a pulsing dot while reviews are running. Animations stop when your system asks for reduced motion.

### Operations

**Operations**, below **Live** in the sidebar, shows CI and security alerts on every connected repository. After each sync, and after **Sync now**, Orbit reads the checks on each repository's default branch and on the latest commit of each open pull request, one repository at a time, then tells each project's agent what fails (see [Telling agents](#telling-agents)). Orbit's own `orbit/review` check is left out, and skipped or cancelled checks count neither way. A commit with more checks than Orbit reads in one sync (100 per commit, and up to 2,000 more per repository) is never counted as passing: its row says Orbit couldn't read every check. The page lists:

- **Open problems**: each thing failing now, with the checks that fail, for a default branch what broke it (see [What broke it](#what-broke-it)), why each failed (see [Why it failed](#why-it-failed)), whether one looks like bad luck and what Orbit did about it (see [Rerunning bad luck](#rerunning-bad-luck)) and its latest events, such as "Failing on main at 7d41e09: CI / Rust tests." and "Still failing at 9f3c2a1.". A failing default branch comes first and **needs you**, as do runs waiting for a runner (see [Runners](#runners)). A failing pull request is its author's to fix, so it is only listed, and only for pull requests Orbit follows: open, not drafts, and from a source Autopilot trusts. So is one whose checks passed before its base branch moved on (see [The base moved](#the-base-moved)). Select one to open the pull request. With nothing failing, it reads **All clear** once Orbit has read every check on each repository's default branch and on the pull requests it follows, and otherwise says on how many repositories it couldn't;
- **Ready to merge**: each pull request Orbit reviews automatically, and whether it is clear to merge, or else the first thing in the way (see [Clear to merge](#clear-to-merge)). Select one to open the pull request;
- **CI by repository**: where each default branch stands (passing, failing, running, or no checks), how many followed pull requests fail, whether CI runs on its pull requests (see [Repositories with no CI](#repositories-with-no-ci)), and when Orbit last read it. When GitHub doesn't answer, the row says why and keeps what Orbit read before; a problem never closes because a read failed;
- **Security alerts**: each repository's open Dependabot, code scanning and secret scanning alerts, and what Orbit can't see (see [Security alerts](#security-alerts)). This shows while **Watch CI** is off too;
- **Self-hosted runners**: the runners Orbit sees for each repository and for each organization that owns one, and where it can't see them (see [Runners](#runners));
- **Actions storage**: each repository's caches and the artifacts it keeps (see [Actions storage](#actions-storage));
- **Recently closed**: problems from the last two weeks, with why each closed. A problem stays longer while Orbit's issue about it is still open, or while its pull request is, so the comment keeps its item numbers;
- **What Orbit did on GitHub**: Orbit's latest 10 comments, issues, edits and reruns on GitHub, newest first, with any that didn't go through and why. Select one to open it on GitHub.

A problem closes when its commit passes ("Passed at 4be81aa."), when its pull request closes, goes back to draft or is no longer followed, when its repository is disconnected, or when **Watch CI** is turned off. A failing default branch tells you once, on the **Needs you** list, by notification and in a note from Bit. While it stays red, it tells you again only when a newer commit fails more checks, or a day later. If it passes and fails again within an hour of telling you, it is the same problem back, so it stays quiet and tells you again only if it is still failing an hour after the last time. A default branch that fails again later is a new problem. The sidebar's **Operations** item shows a red dot while a default branch fails or a security alert needs you, and an amber one while runs wait for a runner. Turn off **Watch CI** in Settings to stop reading CI; the page then offers **Turn on**.

#### Telling agents

While Bit is on, Orbit tells each project's agent on GitHub when its CI fails, where the agent already looks, so it can fix it, and about the security alerts in a private repository (see [Security alerts](#security-alerts)). Each open problem on **Operations** says where Orbit told the agent, with **Open on GitHub**, or why it hasn't.

- **A pull request whose checks fail** gets one comment from Orbit, headed **Orbit · CI**, or **Orbit** with the checks under **CI** on a pull request Orbit reviews automatically, where the comment starts with whether it is clear to merge (see [Clear to merge](#clear-to-merge)). Orbit edits that same comment as the checks change, rather than adding new ones, and it says so when every check passes. Orbit never pushes to the pull request's branch. A pull request whose checks passed before its base moved on gets an item on the same comment (see [The base moved](#the-base-moved)).
- **A default branch whose checks fail** gets an issue titled, for example, "CI is failing on main", labelled `orbit`. Orbit edits it as CI changes, and once Orbit has found what broke the branch, the issue says so (see [What broke it](#what-broke-it)). Once every check passes, the issue says so at once, and Orbit closes it with a short note after the branch has stayed green for an hour, so a flaky branch doesn't close and reopen it on every run. A default branch that fails again later gets a new issue. When a problem closes for another reason, such as **Watch CI** being turned off or the repository being disconnected, Orbit closes its issue at once with a note saying why.
- Each failing check is an item, such as **I1** · `CI / Rust tests` · failing since `7d41e09`, still at `9f3c2a1`, with a link to the check. Under each of the first 5 failing items is why it failed, once Orbit has read it: the step that failed and the end of its log (see [Why it failed](#why-it-failed)). Items are numbered once in their comment or issue and never reused, so an agent can name them. Only a passing commit marks a check as no longer failing: when Orbit stops watching for another reason, items keep what it last saw, such as "failing at `9f3c2a1` when Orbit stopped watching". A hidden `orbit-ops` line at the end holds the same items for agents that read the raw text.

Two rules in **Settings → Buddy → What Bit can do** decide what Bit posts: **Comment on pull requests** and **Open issues**, both on **Do it** to start with (two more decide reruns, see [Rerunning bad luck](#rerunning-bad-luck), and one proposing CI, see [Repositories with no CI](#repositories-with-no-ci)). With **Ask first**, the problem on **Operations** has a **Post comment** or **Open issue** button instead. With **Leave it to me**, or with Bit off, nothing new is posted, nor for a repository Orbit no longer watches because it was disconnected or **Watch CI** is off. Orbit checks all of this again just before each post, so a change you make while it is busy counts at once. Once a comment or issue is on GitHub, Orbit keeps it true whatever the rule, until it closes. If someone deletes it, Orbit leaves it be, and doesn't post a pull request's comment again if that pull request fails later.

The agent answers on the same comment or issue, one line per item, as the [reply skill](#replies-from-coding-agents) teaches: `Taking I2`, `Fixed I2 in <commit>`, `Declined I2: <reason>` or `Need a person on I2: <question>`. Orbit reads the replies after each CI read, only from accounts it trusts (you, your trusted accounts, and the pull request's author when Autopilot trusts its source), and only while the item is failing. A reply counts only if it was posted after Orbit's comment or issue showed the item failing, and the latest answer about an item counts. A check that fails again later starts over: replies posted before Orbit's text shows it failing again are about the failure before, even if they are edited later. However long the conversation, Orbit reads it to the end, up to a thousand comments each time it reads CI. Orbit reads each reply once, so an agent changes its answer with a new reply, not an edit. Each open problem on **Operations** lists the answers, and Orbit's comment or issue says what it found, linking the reply rather than repeating it:

- **Taking**: someone is on it.
- **Fixed in a commit**: Orbit checks that the commit is on the branch, then reads that check on it, and says whether it passes there, still fails, isn't on the branch, or can't be found. While the commit's checks still run, while Orbit couldn't read all of them, or while the commit is newer than the one Orbit last read, it waits. Orbit checks up to 5 fixes after each sync, taking turns. A problem still closes only when its checks pass on the branch's latest commit.
- **Declined**: the agent says it isn't the branch's to fix, such as a broken runner. Bit tells you, with the reason.
- **Need a person**: the question goes on your **Needs you** list until the check passes or the agent answers that item again.

#### Why it failed

After each CI read, Orbit reads why the checks on open problems failed, a few each time (3), new ones first and default branches first, and each check once. For a GitHub Actions job it reads the step that most likely failed the job, such as "Run cargo test", and the end of that step's log: the last lines up to its last error, at most 20 lines of at most 240 characters, without times or colours. A step that logged an error but passed isn't the one shown, nor, usually, one that was allowed to fail, as GitHub doesn't say which those were. Only lines Orbit can be sure are that step's are shown: GitHub times steps only to the second, so a second another step also ran in is left out, apart from the failed command's own last line. The commands GitHub echoes before running them are left out too. Each open problem on **Operations** shows them, and Orbit's comment or issue shows them under the item as plain text, so an agent can start fixing without opening the log. What a log says is shown only as code, never as links, pictures or mentions.

- A step whose log looks like it holds a secret anywhere, such as a GitHub token or a private key, is held back, even if the secret is outside the lines Orbit would show: the item says Orbit held back its log, and Orbit keeps none of it.
- A check that isn't a GitHub Actions job, such as one from another CI service, says Orbit can only read the logs of GitHub Actions jobs. One whose log GitHub no longer has, or that Orbit's GitHub sign-in can't read, says so. Orbit doesn't ask again for these.
- When GitHub doesn't answer, or says Orbit is asking too often, Operations says so and Orbit tries again after a minute, then waits twice as long after each failure, up to an hour. Meanwhile it reads the other checks. After GitHub's rate limit, it reads no more logs until the wait is over.
- A step that couldn't upload an artifact because the account's artifact storage is full ("Artifact storage quota has been hit") says so, and that only a person can fix it: let old artifacts expire, delete some, or raise the account's storage. GitHub counts it again every 6 to 12 hours. It isn't bad luck, even if a network error came first, so Orbit doesn't rerun it (see [Actions storage](#actions-storage)).

Orbit forgets what it read once the check stops failing or its problem closes. A check that fails again on a newer commit is a new job, so Orbit reads it afresh.

#### Rerunning bad luck

Some failures aren't the code's fault, and running the job again fixes them. From what it read about a GitHub Actions job, Orbit says a failure looks like bad luck when:

- no step failed, as when the runner was lost;
- the runner went away, such as "The runner has received a shutdown signal";
- the job failed setting up, or getting the code or tools ready (`actions/checkout`, `actions/setup-…` or `actions/cache`), before any test ran;
- the failed step's own lines show a network or registry error, such as `ECONNRESET`, "Could not resolve host", "502 Bad Gateway" or Docker's `toomanyrequests`. The command and settings GitHub shows at the start of a step don't count, as they say what the step ran, not what happened.

Each open problem on **Operations** shows "Looks like bad luck:" and the reason. Two rules in **Settings → Buddy → What Bit can do** decide what Bit does about it:

- **Rerun a failed job once when it looks flaky**, on **Do it** to start with: Orbit reruns the job once, and its comment or issue says so under the item, such as "Orbit reran this job, as it looks like bad luck: The runner went away." Orbit reruns before it posts, so the agent knows not to chase it.
- **Rerun the same job a second time**, on **Ask first** to start with: when the rerun fails again and still looks like bad luck, Bit tells you, and the problem on **Operations** has a **Rerun again** button. With **Do it**, Orbit reruns it a second time on its own.

A job that fails again after a rerun is likely real: the item says "It failed again after it was rerun, so it's likely real.", and Bit tells you. Orbit never reruns a check more than twice on one commit, whatever the rules; a newer commit starts again. It reruns only jobs on the latest commit of a default branch or of a pull request it follows, while Bit is on, and with **Ask first** on the first rule, Operations has a **Rerun** button instead. A check that passes after a rerun says "passes at `9f3c2a1` after a rerun".

A rerun is a write on GitHub, so it goes in Orbit's journal like a comment (see below). Orbit saves that it is about to rerun a job before it asks GitHub. If Orbit stops before GitHub answers, the rerun may have gone through, so Operations says "Orbit is checking on GitHub whether its rerun went through." and Orbit looks on GitHub before it does anything else about that check.

Before each rerun, Orbit looks at the job on GitHub. It must be the failing check's job at the commit Orbit read, or Orbit won't rerun it and says why. Then Orbit reads every job in the job's workflow run: if a newer try of the job is already there, such as one an earlier try made just before Orbit stopped, it counts that, and the item says "This job was already rerun on GitHub." GitHub reruns a job only once its whole workflow run has finished, so while other jobs in it still run, Orbit waits and tries again later. When GitHub refuses a rerun, such as when Orbit's sign-in may not rerun jobs there, Operations says why and Orbit doesn't ask again for that job. When GitHub doesn't answer, or its list of the run's jobs is cut short, Orbit tries again after a minute, then waits twice as long each time, up to an hour.

Orbit doesn't yet treat a check that passed earlier on the same commit as bad luck.

#### What broke it

When a default branch starts failing, Orbit reads the branch's history on GitHub once, back from the first commit it saw failing, to the newest commit whose checks passed. The commits after that one, up to the oldest that failed, are where it broke. A commit whose checks didn't run or finish in between counts too, as it may have broken it. Orbit follows the branch's own commits, one per change that landed, not the commits a merge brought with it, and looks back at most 20 commits.

- When those commits are one pull request's merge, Orbit's issue, the problem on **Operations** and the **Needs you** item say so, such as "It started failing with #42, merged as `7d41e09`; `3b8e6f4` before it passed." A pull request rebased onto the branch counts as one merge too.
- Otherwise they list where the commits came from, such as "It started failing in the 3 commits after `3b8e6f4`, which passed, up to `7d41e09`: from #41, #42 and 1 pushed without a pull request."
- With no commit that passed in the 20 before, or none since the branch began, Operations says so and the issue names nothing. When GitHub doesn't answer, Operations says so and Orbit tries again after a minute, then waits twice as long each time, up to an hour.
- When the branch passes and fails again within the hour, it is the same problem back, but something new broke it, so Orbit reads again from the new failure and the issue says what broke it this time.

Orbit's own `orbit/review` check doesn't count, and a commit whose checks Orbit couldn't all read never counts as passing.

Every write Orbit makes on GitHub is saved in its journal before it is sent. If Orbit stops before GitHub answers, it looks on GitHub for what it sent before doing anything else about that problem, even if CI has passed or your rule changed since, and keeps what it finds up to date, so nothing is posted twice. If there are too many comments or issues to look through them all, Orbit posts nothing and says so. When GitHub doesn't answer, Orbit tries again after a minute, then waits twice as long after each failure, up to an hour, and the problem says what went wrong. Looking on GitHub in between doesn't start the wait over.

#### The base moved

A pull request's checks run on the pull request merged into its base branch as the base was when they began. When the base moves on afterwards, such as when another pull request merges, those checks never ran with its new changes, so two pull requests that pass apart can break together. After each CI read, Orbit compares when each followed pull request's passing checks began with when its base's latest commit was made, for pull requests into the default branch.

- When the base's latest commit is newer, the pull request is listed on **Operations**, for example "Its checks pass, but main moved on to 9f3c2a1 after they began. Its author's to merge it in and let CI run again." Orbit's comment on the pull request, the same one it uses for failing checks and under the same rule, says "Every check passes at `5e6f7a8`, but `main` has moved on since they began. Merge `main` in and let CI run again, so they run with its latest changes.", with an item such as **I3** · `main` moved on since the checks at `5e6f7a8` began.
- The agent answers it like any other item. Orbit doesn't check a fix for it: it is done once the pull request's checks pass again having begun after the base's latest commit. If they fail instead, the failing checks are the problem. If the base moves on again later, the same item comes back. If the base itself changes, such as when the default branch is renamed and GitHub moves open pull requests onto the new name, the item names the new branch.
- When a check doesn't say when it began, or GitHub doesn't say when the base's latest commit was made, Orbit says nothing. For a commit status from another service, Orbit goes by when it was last set, which can be after it began, and for the base by when its latest commit was made, which for a commit pushed long after it was made is earlier than when it landed, so Orbit may miss a base that moved on then.

#### Repositories with no CI

A repository where nothing checks a pull request before it merges gets a note on **Operations**, and Bit can ask its agent to add CI. After each CI read, Orbit reads each repository's files once for each new commit on its default branch, two repositories at a time: the workflows in `.github/workflows`, the names at the top of the repository, and its `package.json`.

- **A workflow runs on pull requests** when its `on:` names `pull_request` or `pull_request_target`, or `push` for every branch, for branches but some (`branches-ignore`), or for a pattern such as `feature/**`. One that runs on `push` only to branches it names, such as `main`, or only for tags, doesn't, nor does one that only runs on a schedule, by hand or for a release.
- **Orbit says no CI runs only when it is sure.** A workflow it can't read for sure counts as CI, and so do another CI service's files at the top, such as `.circleci`, `.travis.yml`, `.gitlab-ci.yml` or a `Jenkinsfile`. So do checks on the latest commit of a pull request Orbit follows, and, in a repository with no workflows at all, checks on the default branch: something runs them. Once Orbit has seen such checks, it remembers that for the repository. And it waits 10 minutes after reading a commit, so another service's checks have time to show up.
- The repository's row under **CI by repository** then says, for example, "No CI runs on its pull requests. None of its workflows on main runs on pull requests, at e7a51c3. Orbit can propose one built from package.json and package-lock.json." It isn't an open problem and doesn't need you, as nothing fails.

The rule **Propose CI where none runs on pull requests** in **Settings → Buddy → What Bit can do** decides what Bit does, on **Ask first** to start with: Bit tells you once in a note, and the row has a **Propose CI** button. With **Do it**, Orbit opens the issue on its own; with **Leave it to me**, or with Bit off, it opens none.

- **The issue** is titled "Add CI that runs on pull requests" and labelled `orbit`, like a failing branch's. It says what Orbit found and has one item, **I1** · add a workflow that runs on pull requests, which the agent answers like any other. Orbit doesn't check a commit the agent names: it waits to see the workflow on the default branch.
- **The workflow to start from**, as `.github/workflows/ci.yml`: one job on GitHub's Ubuntu runner, on every pull request and each push to the default branch, using only GitHub's own actions. For a `package.json`, it uses the package manager its lock file names (npm, pnpm or Yarn), the Node version in `.nvmrc` or `.node-version` or else the latest long-term one, installs, then runs the `lint`, `typecheck`, `build` and `test` scripts it has, or else `check`. A `Cargo.toml` gets `cargo test`, and a `go.mod` gets `go test ./...`. Its commands come from Orbit's own list, never from the repository's files, so the issue asks the agent to check they're the ones the project uses. When Orbit can't tell how the repository builds, such as one that only uses Bun, the issue just asks for a workflow.
- Orbit never adds the workflow itself, nor makes it a required check. It edits the issue as it reads newer commits, and once a workflow on the default branch runs on pull requests, it closes the issue as done, saying for example "A workflow on main runs on pull requests at 9f3c2a1, so Orbit is closing this issue." It closes it at once otherwise too, saying why, such as when checks ran on a pull request, Orbit can no longer tell, the repository is disconnected or **Watch CI** is turned off.

#### Runners

A job runs on a runner: one of GitHub's own, or a self-hosted one that a repository or its organization added. After each CI read, Orbit reads the self-hosted runners of each connected repository and of each organization that owns one, and each repository's queued runs made more than 30 minutes ago, up to 300, a few places at a time, each one again every 5 minutes.

- **Runs waiting for a runner**: a run queued for more than 30 minutes is waiting for a runner, such as one whose only runner is offline, or one that asks for a label no runner has. It becomes an open problem on **Operations** and a **Needs you** item, such as "A run of Release in acme/api has waited more than 30 minutes for a runner. Its one self-hosted runner is offline. The acme organization shares 2 self-hosted runners (1 online and free), but Orbit can't tell whether acme/api may use them.", with **Open run** for the one that has waited longest. It says what Orbit sees of the repository's own runners and of those its organization shares. A runner group can keep an organization's runners from some of its repositories, and Orbit can't tell which, so it never counts them as the repository's. It also says when it read only part of a list, or when its latest read failed and what it says may be out of date. It tells you like a failing default branch does: once, again a day later if runs still wait, and quietly when it clears and comes back within an hour. It closes once Orbit has read every queued run and none has waited that long ("No runs wait for a runner now."). When more are queued than Orbit reads, it says "Orbit couldn't read every queued run, so more may wait." and stays open. Only a person can add or fix a runner, so Orbit tells no agent about it and posts nothing on GitHub.
- **Self-hosted runners** on **Operations** lists each place that has some, with how many are online and busy, and each runner's name, system, state and labels, up to 10 a place. An organization's row says Orbit can't tell which of its repositories may use them. Places with none aren't listed; when none has any, it says so.
- **Where Orbit can't see them**, it says so rather than saying there are none. A repository's runners need admin rights on it, and the runners an organization shares need an owner of the organization signed in with the `admin:org` scope (`gh auth refresh --scopes admin:org`). A person's account shares no runners, so Orbit reads it only once, to learn that it is one.
- When GitHub doesn't answer, the place says why and keeps what Orbit read before, and its problem stays as it was. Orbit tries again after a minute, then waits twice as long each time, up to an hour.

Orbit doesn't yet say which label a waiting job asks for, nor read runners an enterprise shares.

#### Actions storage

GitHub Actions keeps two kinds of files for a repository. Caches speed up builds; past a repository's limit, usually 10 GB, GitHub drops the ones used least. Artifacts are files a workflow uploads; in a private repository they count toward the account's storage, and once that is full no job can upload more. After each CI read, Orbit reads each connected repository's caches and the artifacts it keeps, a few repositories at a time, each one again every 30 minutes.

- **Actions storage** on **Operations** lists each repository with how many caches it has and how much they hold, and how many artifacts it keeps that haven't expired and how much they hold, such as "118 caches holding 9.6 GB, near GitHub's usual 10 GB limit." and "52 artifacts kept, holding 635 MB.". Orbit reads the newest 300 artifacts; when there are more, it says so.
- **Near the limit**: caches holding 9 GB or more are near GitHub's usual limit. The row says so, as GitHub soon drops the ones used least, which can slow builds, and Bit says so when you ask how your repositories are. A repository can have a higher limit, which Orbit can't see.
- **Full**: when a job failed because the account's artifact storage is full (see [Why it failed](#why-it-failed)), its repository's row says so.
- Orbit can't see an account's storage or its limit, as that is billing, which Orbit doesn't read. Where GitHub won't show a repository's storage to Orbit's sign-in, the row says so. When GitHub doesn't answer, the row says why and keeps what Orbit read before, and Orbit tries again after a minute, then waits twice as long each time, up to an hour.

Orbit only reads storage: it never deletes a cache or an artifact.

#### Security alerts

GitHub raises three kinds of security alerts on a repository: Dependabot alerts for a dependency with a known weakness, code scanning alerts for code a tool such as CodeQL flagged, and secret scanning alerts for a secret, such as a token, committed to it. After each sync, Orbit reads each connected repository's open alerts of every kind, the newest 100 of each, two repositories at a time, each one again every 30 minutes. It does so whether or not **Watch CI** is on.

- **Security alerts** on **Operations** lists each repository with each kind's open alerts, such as "Dependabot alerts: 2 open alerts, 1 high or critical.", and the first 5 of each, most severe first, with their severity and where they are, such as "lodash in package-lock.json: Prototype pollution in lodash. Fixed in 4.17.21." or "GitHub Personal Access Token in src/config.ts line 4. GitHub says it still works.". When GitHub has more open than the newest 100, it says so. **Open on GitHub** opens the repository's security page.
- **What Orbit can't see**: a kind that's off says so, such as "Dependabot alerts are off for acme/docs." or "No code scanning runs on acme/api.". GitHub shows alerts only to a repository's admins and security managers, so where Orbit's sign-in is neither, the row says "GitHub doesn't show acme/dashboard's secret scanning alerts to Orbit's sign-in. It shows them to admins and security managers.". When GitHub doesn't answer, the row says why and keeps what Orbit read before, and Orbit tries again after a minute, then waits twice as long each time, up to an hour. After GitHub's rate limit, it reads no more alerts until the next sync.
- **What needs you**: a secret GitHub found, and a Dependabot or code scanning alert that is high or critical. Each one tells you once, on the **Needs you** list, by notification and in a note from Bit, such as "acme/api has a high or critical Dependabot alert: lodash (critical)." or "GitHub found a secret in acme/api: a GitHub Personal Access Token in src/config.ts line 4. Revoke it, then close its alert on GitHub.". A new one tells you again. A code scanning alert with no security severity counts as medium when its tool calls it an error, and low otherwise. The rest are only listed.
- **When one goes**: each repository has an open problem for each kind with open alerts, with what changed, such as "New: lodash (critical) and qs (low).". When an alert leaves GitHub's list of open ones, Orbit looks it up, up to 10 a repository each time, those it hasn't asked about first and then those it asked about longest ago, and lets it go only once GitHub says what became of it, such as "Dependabot alert 12, lodash: fixed, GitHub says." or "Secret scanning alert 3, GitHub Personal Access Token: closed as revoked by @sam.". The problem closes once its last alert goes. When a kind is turned off on GitHub, or GitHub stops showing it to Orbit, its problem closes, saying Orbit can't see what became of its alerts. Turning off **Watch security alerts** in Settings, or disconnecting the repository, closes them too.

- **Telling the agent**: in a private repository, Orbit opens an issue for each kind of Dependabot and code scanning alerts, titled, for example, "Fix the open Dependabot alerts", labelled `orbit`, so the repository's agent can fix them (see [Telling agents](#telling-agents)). The issue starts "GitHub has 2 open Dependabot alerts here, 1 of them high or critical." and lists each alert as an item, such as **I1** · Dependabot alert 14 · `lodash` (critical) in `package-lock.json` · update `lodash` to `4.17.21` or later, with a link to the alert. A dependency no version fixes yet says to replace it, or to ask a person whether to dismiss the alert. Orbit edits the issue as alerts come and go, and closes it with a short note once none is open: as done when GitHub says each one is fixed or dismissed, or saying Orbit can't say when GitHub no longer has some of them without saying what became of them. The same **Open issues** rule decides whether Bit opens it, and with **Ask first**, its kind on **Operations** has an **Open issue** button; once open, **See issue** opens it. Orbit doesn't tell an agent about a secret, as only a person can revoke one, nor about any alert in a repository that isn't private, as an issue there would show others where the weakness is: those stay with you. If GitHub later says the repository isn't private, Orbit adds nothing new to its issue, and an issue Bit was asking about doesn't open, until GitHub says it is private again. If GitHub doesn't answer whether it is private, Orbit reads none of its alerts until it does.
- **What the agent says**: it answers on the issue as it does for CI: `Taking I1`, `Fixed I1 in <commit>`, `Declined I1: <reason>` or `Need a person on I1: <question>`. An alert counts as fixed only once GitHub marks it fixed, which can wait until GitHub checks the default branch again, so a fix says Orbit is waiting for GitHub to mark the alert fixed. When the agent declines a high or critical alert, it's yours to decide: it goes on your **Needs you** list, to have it fixed or dismiss its alert on GitHub. A declined lower one is noted by Bit. An alert that opens again starts over, so what was said about it before no longer counts.

Secrets stay secret: Orbit asks GitHub to leave out each secret's value and never reads it, so no secret is kept, shown or sent anywhere. Orbit never closes or dismisses an alert: that is a person's call.

#### Clear to merge

Orbit reviews a pull request automatically when it is open, not a draft, from a source Autopilot trusts, in a repository with **Automatic reviews** on. For each one, Orbit works out whether it is clear to merge at its latest commit. It is clear only when all of these hold:

- Orbit's review loop on it is **Clean**.
- Every check on its latest commit passed, and Orbit read them all. In a repository where no CI runs on pull requests, there are no checks to wait for, but only once **Operations** says so for the default branch's latest commit (see [Repositories with no CI](#repositories-with-no-ci)), after Orbit's 10-minute wait for another service's checks.
- Its checks began after its base branch's latest commit was made, so they ran with it (see [The base moved](#the-base-moved)), and Orbit's review was with that same commit. When main has moved on but GitHub still shows the pull request on the older commit, Orbit says so and waits for a review with the latest main. Orbit only follows each repository's default branch, so a pull request into another branch is never clear, and neither is one whose checks don't say when they began.
- GitHub says it can merge. Orbit asks GitHub only once nothing else is in the way, a few pull requests each sync, and again when its latest commit or its base's latest commit changes, or 10 minutes later, since a rule or a review on GitHub can change the answer. While GitHub is still working it out, or doesn't answer, Orbit asks again after a minute, then waits twice as long each time, up to an hour, and what GitHub said before about the same commits stands meanwhile.
- Where its base branch's rules need approvals, enough people with write access approved its latest commit, and nobody's request for changes stands. An approval of an older commit doesn't count, even where GitHub would still count it.

Otherwise Orbit says what is in the way, first things first, such as "Orbit is reviewing abc1234.", "2 blocking findings from Orbit's review of abc1234 wait on the author.", "Checks are still running on abc1234.", "main moved on since its checks ran: merge main in and let them run again.", "GitHub is still checking whether it merges." or "It conflicts with main: merge main in and fix the conflicts.". When GitHub says a rule blocks it, such as a required review, Orbit says so. While **Watch CI** is off, Orbit can't see the checks, so nothing is clear.

When it asks GitHub, Orbit also reads who was asked to review the pull request, their reviews, and its base branch's rules. Where the rules need approvals, the answer names who it waits for, such as "main needs 1 approval of abc1234 and it has none: alice approved 9f8e7d6 but not abc1234.". A request for changes on the latest commit is the author's to answer; one on an older commit waits for its reviewer to look again. Only reviews from people with write access count, as on GitHub, so Orbit asks GitHub about each approval of the latest commit and each request for changes, up to 10 people: an approval it can't confirm doesn't count, and a request for changes it can't rule out stands. A comment doesn't change where a reviewer stands, and Orbit leaves a code owner's approval to GitHub. When Orbit can't read all the reviews (it reads up to 300), it isn't clear until it can, though GitHub's own answer always stands. A branch with more rules than Orbit reads counts as one whose rules it can't see. Reading a branch's protection takes an admin's sign-in, so when Orbit can't see the rules, it says so and leaves the reviews to GitHub: a clear answer ends "Orbit can't see main's rules, so it left its reviews to GitHub.", and if GitHub says a rule blocks it, Orbit names those who haven't approved the latest commit. The **Merging** panel lists each reviewer and where they stand, and what the rules need.

The answer shows under **Ready to merge** on **Operations** and in the **Merging** panel on the pull request's **Overview**. Bit gives it too: **How are my loops?** adds it after each clean loop, such as "orbit#145: round 3 of 4, clean. Not clear to merge yet. main moved on since its checks ran: merge main in and let them run again.", and a typed question such as "Can I merge orbit#145?" gets it. Orbit never merges a pull request itself: clear to merge is what it tells you and the pull request's agent.

Orbit tells the agent at the top of its one comment on the pull request, headed **Orbit**, such as "**Clear to merge** at `abc1234` against `main` at `def5678`." or "**Not clear to merge yet.** It conflicts with main: merge main in and fix the conflicts.". A hidden `orbit-ready` line under it names the commit and base it was worked out for, so an agent can tell a stale answer from a fresh one, and whose move it is: the author's, a person's (you, or whoever a rule on GitHub waits for), or nobody's yet while Orbit, CI or GitHub is still at work. A failing check's items go under **CI** on the same comment (see [Telling agents](#telling-agents)).

- Orbit posts the comment once the pull request is clear or waits on someone, not while Orbit, CI or GitHub is still at work.
- It edits the comment only when the answer changes: it turns clear or not, its latest commit or its base's moves, or it becomes someone else's move. Each change wakes the agent once, rather than at every step in between; the words catch up whenever the comment changes anyway.
- When Orbit stops working it out for a pull request that is still open, such as while it is a draft, the comment says so. A closed pull request's comment is left as it was. If it is reopened, the comment gives the answer again, and the CI part from before it closed goes until a check fails again.
- Orbit works the answer out again just before each change to the comment, so one that went stale while Orbit waited on GitHub is never sent.
- **A pull request left waiting gets one nudge** for whoever owes the next step, once its answer has stood unchanged for a while, as Orbit counts changes above:
  - On the agent, such as for findings, a failing check, a conflict or a reviewer's request for changes: 2 hours after the comment first showed it (or after Orbit found it, when GitHub didn't answer its post), Orbit edits the comment once more, adding "**Reminder:** it has waited on the author for more than 2 hours.". It doesn't remind the agent about failing checks or the base moving on once the agent declined, or asked a person about, each item that fails. A question already goes on your **Needs you** list.
  - On a person, a reviewer or a rule on GitHub: a day after it started, whether or not Orbit has posted its comment yet, the pull request goes on your **Needs you** list, such as "It has waited on a person for more than a day. main needs 1 approval of abc1234 and it has none: bob hasn't reviewed abc1234 yet.", with **Open on GitHub**. The comment isn't edited for it. Orbit's loop needing you, its checks while **Watch CI** is off, and a base branch Orbit can't judge are on screen already, so they get no nudge.
  - Each wait is nudged once. A push, or a change of whose move it is, starts it again. Nothing is nudged while **Watch CI** is off. The **Merging** panel says when Orbit reminded the agent, once GitHub has the edit, or put the pull request in **Needs you**.
- The **Comment on pull requests** rule decides whether Bit posts it, as for failing CI. With **Ask first**, the answer goes only on a comment you let Bit post. Nothing new is posted while **Watch CI** is off, but a comment already on GitHub then says Orbit can't see the checks.
- **Open Orbit's comment** in the **Merging** panel opens it on GitHub.
- Anyone can post a comment that looks like Orbit's, so the `answer-orbit-reviews` skill tells the agent to read the answer only from Orbit's own comment, posted by the GitHub account Orbit signs in with, and to check GitHub again right before it merges. Tell your agent which account that is, such as in its instructions. Clear to merge never lets an agent merge on its own: it merges only where you already let it.

### Review loops

**Review loops** sits below **Operations** in the sidebar. While Autopilot is on, the header reads, for example, "Autopilot · 3 loops running · 1 needs you". The **Review loops** page lists:

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

Orbit reads each connected repository's 100 most recently updated pull requests in the background: when it starts, every five minutes, and as soon as you connect a repository. It keeps them in a `ledger` folder in its app-data folder, separate from its saved state, and removes a repository's history when you disconnect it. Each read fetches only the pull requests GitHub says have changed, and once an hour every open one, since GitHub doesn't always count an edited comment as a change; if that hourly read fails, the next read tries it again. **Read again** starts a read now that fetches every open one too. While a read runs, the page says which repository it is reading; a repository GitHub refused shows why, keeps what Orbit already had, and is tried again on the next read.

**Export** saves the page as one Markdown file to keep or share outside Orbit: what is worth a look, the rulebook by area, and each pull request's chapter with its write-up, its findings and what is worth a look in it. It covers the repository chosen in the repository filter, or every repository. Text copied from pull requests appears as it was written, so any pictures, links or HTML in it show as plain text. On the desktop you choose where to save it; in a browser it downloads, named like `orbit-decisions-acme-api-2026-10-12.md`.

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

When something can only move forward with you, the sidebar shows **Needs you** with a count, and the **Review loops** page lists each item at the top with its reason and one main action: **Approve or deny** for an approval, **Publish** for a held report or one waiting for you, **Continue loop** when the round limit is reached, **Check again** for a connection, **Open repositories** or **Open settings** for sync and save problems, **Open operations** for failing CI, runs waiting for a runner and security alerts, **Open on GitHub** for a pull request left waiting on a person, **Post here again** where another Orbit posts, **Got it** for Bit's morning brief, and **Open chat** otherwise. Selecting the item itself opens where it is resolved: the review's chat, the PR, **Repositories**, **Operations**, **Settings**, or Bit's page for the brief. An approval item opens that exact request. An item appears when:

- a review is waiting for your approval;
- an automatic report was held instead of published, with the reason;
- an automatic review failed, or was interrupted by a restart;
- Orbit couldn't start an automatic review, such as when the repository's local checkout is gone, with the reason; it tries again every minute;
- the loop needs you for another reason from the list above;
- Autopilot was turned off because Orbit could not save its state (turn Autopilot back on once saving works);
- a repository has failed to sync 3 times in a row, or Orbit could not sync with GitHub at all 3 times in a row (for example, when the GitHub sign-in expired). When the GitHub CLI isn't installed or isn't signed in, this shows after the first sync, and Orbit doesn't try to start `gh` until it is installed. The item says what is wrong and how often Orbit checks again;
- Codex, Claude, Antigravity, or Docker and the Dev Container CLI isn't connected while a connected repository uses it, for example after its sign-in expired. Sign in again, then tap **Check again**;
- a connected repository's default branch fails its CI checks, with the checks that fail and since which commit (see [Operations](#operations) for when it tells you again);
- a project's agent asks for a person about a failing check or a security alert (`Need a person on I2: …`), with its question, until the check passes, the alert is fixed, or the agent answers again (see [Telling agents](#telling-agents));
- a project's agent declined a high or critical security alert, with its reason, as only you may leave one open (see [Security alerts](#security-alerts));
- a repository's runs have waited more than 30 minutes for a runner, with what Orbit sees of its runners (see [Runners](#runners));
- GitHub found a secret in a connected repository, or raised a high or critical Dependabot or code scanning alert, with what and where (see [Security alerts](#security-alerts)). A new one tells you again;
- a pull request Orbit reviews automatically has waited on a person, a reviewer or a rule on GitHub, for more than a day, with what it waits for (see [Clear to merge](#clear-to-merge));
- another Orbit posted on a pull request after this one did, so this one stopped reviewing and posting there (see [One Orbit at a time](#one-orbit-at-a-time));
- Bit's morning brief came, until you tap **Got it** or the next brief's time comes (see [Morning brief](#morning-brief)). It comes first in the list, and doesn't make Bit worried.

An item disappears once it is resolved. A stopped loop lists only its approvals, since you have already taken it over.

Orbit also shows a desktop notification once when an item first appears, and not again while it stays open, even after a restart. A follow-up chat on the review hides the item until the chat ends, and it isn't announced again afterwards. When Orbit comes to the front after a notification, it opens that item. A browser using Orbit can show them too once you turn them on in it, even with its page closed (see [Use Orbit from your phone or laptop](#use-orbit-from-your-phone-or-laptop)). Turn off **Notify me when a review needs attention** in Settings to stop notifications everywhere; the list and count still appear.

### Bit, your buddy

Bit is Orbit's buddy: a little character with a face, at the bottom of the sidebar. It shows its latest note, or just its face when the sidebar is collapsed. Select it to open its page: one conversation with Bit, newest at the bottom, that the desktop window and every browser [using Orbit from your phone or laptop](#use-orbit-from-your-phone-or-laptop) share. Bit writes a note when:

- a round starts, and when it finishes, with how many blocking findings it found, or that it failed or had no findings list;
- a loop comes out clean, with its blocking findings round by round, such as `4 → 1 → 0`;
- a loop is stopped or continued;
- something new needs you, with the same reason as the **Needs you** list;
- a repository gets connected, from **Repositories** or one of Bit's buttons, with what it still needs and a button for each (see [Help me set up](#ask-bit));
- Orbit posts a new comment or opens an issue to tell an agent its CI fails, or closes one of its issues; and when an agent declines a failing check, or says it fixed one in a commit where Orbit sees it still fail, that isn't on the branch, or that GitHub doesn't know (see [Telling agents](#telling-agents));
- Orbit reruns a job that looks like bad luck, or the job fails again after a rerun, with a reminder of the **Rerun again** button when Bit asks first (see [Rerunning bad luck](#rerunning-bad-luck));
- a pull request on [Decisions](#decisions) has something to decide: it merged while a finding was still open, the reviewer disputes a finding the builder declined, or its write-up marks a decision as needing one. Bit notes each once, with the writer's reason, and only what turns up after it started watching that repository, so connecting one doesn't retell its history;
- once a day, its morning brief, when there is something to say (see [Morning brief](#morning-brief)).

**Open decisions** under a note opens that pull request's chapter on Decisions; **Open loop** opens that pull request's loop; **Open operations** opens Operations for failing CI; **Open repositories** opens Repositories for a repository's sync problem, and **Open settings** opens Settings for anything else. Bit keeps its last 50 notes, answers included, and they survive a restart. When Orbit starts for the first time with Bit, it looks at the loops already running and starts noting from there, rather than retelling their history.

#### Morning brief

Once a day, at the time you choose under **Settings**, **Buddy**, **Brief time** (09:00 unless you change it), Bit sums up in one note:

- the pull requests Orbit reviews automatically that are clear to merge, with the head and base they're clear at;
- the stuck ones, and whose move it is: those Orbit already reminded the author about, after 2 hours, or put on **Needs you** for a person, after a day (see [Clear to merge](#clear-to-merge));
- the open CI problems: a failing default branch or pull request, checks that ran without the base's latest commit, and a repository with no CI on its pull requests;
- the security alerts that need you: secrets GitHub found, and high or critical Dependabot and code scanning alerts. These need **Watch security alerts** on;
- notes on runners and storage: runs waiting for a runner, a repository's or organization's self-hosted runners that are all offline, and caches near GitHub's usual limit. These need **Watch CI** on, as Orbit only reads runners and storage then.

The note starts with how many of each, then how many lines are new since the last brief, each marked **New**, and how many things from the last brief have settled, such as a check that passes again or a pull request that merged. A day when nothing is new and nothing settled gets no brief. Nor does a first day with nothing to say. The brief also goes on **Needs you**, with a desktop notification, until you tap **Got it** there or under the note, or the next day's brief time comes. If Orbit isn't running at the brief time, the brief comes when it next is, that day. It waits while Orbit's latest sync of your pull requests or read of a repository's CI failed, for any repository, or its latest question to GitHub about whether a pull request merges failed while nothing else is in that pull request's way, so it never sums up what Orbit couldn't read. It comes on the next pass that reads them all, that day; while a repository keeps failing, **Needs you** says so instead. Runners and storage are read less often, so their notes don't wait: they say when Orbit's latest read of them failed and may be out of date, and when GitHub has more runners than Orbit read, they speak only of those Orbit read. Turn **Morning brief** off to stop it; turning Bit or the brief off also takes an open brief off **Needs you**. Bit works it all out from what Orbit already knows, without any AI.

The brief time follows the clock of the computer Orbit runs on, until you choose a **Time zone** under it. Search for yours by region or city, such as London or New York. That matters when Orbit [runs on a computer that stays on](#run-orbit-without-a-window) set to another zone, as many servers keep UTC: in a browser or a desktop app using that Orbit, the list suggests this device's zone, and Settings says when the two differ. Choose **This computer** (**Orbit's computer** away from it) to go back to its clock.

#### Ask Bit

The buttons under the conversation ask Bit a question. It answers straight away from what Orbit knows, without any AI:

- **What needs me?** lists the **Needs you** items.
- **What's running?** lists reviews, syncs and security scans in progress.
- **How are my loops?** lists each open loop with its round and state.
- **How are my repositories?** lists each repository with its open pull requests, whether Autopilot reviews it, any failed syncs, whether CI passes on its default branch, and how many security alerts are open and need you.
- **Help me set up** walks you through getting reviews going, with a button for each step. It stands out until GitHub and Codex are connected and you have a repository.

**Help me set up** starts with what Bit answers your questions with, since it can't help much without it. When that's ready, Bit just says which it is. When it isn't, Bit says why, with a button that fixes it, and:

- If exactly one of Codex, Claude Code and Antigravity is signed in and ready, Bit switches to it on its own and says so in a note; you can change it back under **Settings**, **Buddy**. Bit does the same when you type a question or tap **Check again**. It never switches away from Ollama or an API key, since you chose where your questions go: instead it offers **Answer with Codex** (or whichever is ready).
- If two or three are ready, Bit offers **Answer with** each of them for you to pick.
- If none is, Bit offers a button for every choice: **Sign in to Codex** or **Get Codex CLI**, **Get Claude Code**, **Get Antigravity**, then **Use Ollama**, **Use an OpenAI API key**, **Use an Anthropic API key** and **Use a Gemini API key**, which make it Bit's answerer so you can finish setting it up under **Settings**, **Buddy**.

Then it says whether GitHub and Codex are connected, and Claude, Antigravity, or Docker and the Dev Container CLI while a repository or Bit's answers use them, and Ollama or your API key while it answers Bit, with a button to fix each one that isn't and a **Check again** button for once you have:

- **Sign in to GitHub** or **Sign in to Codex**, when the program is installed but signed out. Orbit starts the sign-in and Bit shows a short code with a button for the website that takes it, which opens on whichever device you tapped from, your phone included. Enter the code there and Bit tells you when you're signed in; the code lasts 15 minutes.
- **Get GitHub CLI**, **Get Codex CLI**, **Get Claude Code**, **Get Antigravity**, **Get Docker**, **Get Dev Container CLI** or **Get Ollama**, when it isn't installed, or for Claude, Antigravity and Ollama whenever they aren't connected. It opens the page that says how to install it and sign in. For an API key that isn't saved or that the provider turned down, **Get an OpenAI API key**, **Get an Anthropic API key** or **Get a Gemini API key** opens the provider's page for keys. Tap **Check again** once you have.

With no repository connected yet, it asks GitHub for your most recently pushed repositories and offers **Connect** for up to 3 of them. While GitHub isn't connected, it says so instead. Then, for each repository:

- **Setup command**: if it has none, Bit looks for a lock file at the top of its copy and offers the command that installs from it, with network access on, since installing downloads packages: `npm ci --ignore-scripts` for `package-lock.json` or `npm-shrinkwrap.json`, `pnpm install --frozen-lockfile --ignore-scripts` for `pnpm-lock.yaml`, `yarn install --frozen-lockfile` for `yarn.lock`, `bun install --frozen-lockfile` for `bun.lock` or `bun.lockb`, `cargo fetch` for `Cargo.lock`, `uv sync --frozen` for `uv.lock`, `poetry install` for `poetry.lock`, `pip install -r requirements.txt` for `requirements.txt`, `go mod download` for `go.mod`, and `bundle install` for `Gemfile.lock`, checked in that order. Claude and Antigravity reviews run a setup command only in a dev container, so Bit offers none for them otherwise. A setup command you already have is kept.
- **Automatic reviews**: if they're off, **Review owner/name automatically** turns them on.

Last, if Autopilot is off for all of Orbit, **Turn on autopilot** turns it on. Each button lists its change and waits for your tap, as other answers' buttons do. Bit works all of this out from Orbit's state and the files in each copy, without any AI, and the answer starts with how many things are left. When everything is done, Bit says you're all set.

When a repository gets connected, Bit follows up on its own with the same steps for just that repository.

Your question and Bit's answer join the conversation, so every window sees them.

You can also type any question in the box below the buttons, such as "Why did orbit#12 stop?" or "Can you check for new pull requests less often?", and press Enter. Bit answers these with the program you choose under **Settings**, **Buddy**, **Who answers your questions**: **Codex** (the default), **Claude Code**, **Antigravity**, **Ollama**, or **OpenAI**, **Anthropic** or **Google Gemini** with an API key of yours. The first three each use their own sign-in, with the model and effort you chose for them as a reviewer. Ollama runs on this computer or another one on your network: give its address (`http://localhost:11434` unless you changed it) and pick one of the models it has, which Orbit lists once it reaches it. For an API key, paste it under **API key** and press **Save key** (**Get a key from…** opens the provider's page for keys), then pick one of the models the provider offers your key, which Orbit lists once the provider accepts it. Orbit keeps the key in your computer's password store (Keychain on macOS, Credential Manager on Windows, the Secret Service on Linux), never in its settings or state, and only ever sends it to that provider; **Remove key** deletes it from the password store. Many servers have no password store, so on [Orbit without a window](#run-orbit-without-a-window) there may be nowhere to keep a key: then `orbit-server` says so as it starts, Settings says so where the key would go, the three API key choices can't be picked, and Bit's help offers Codex, Claude, Antigravity or Ollama instead. If a password store starts later (or you set one up), **Check again** finds it, with no restart. Each question is billed to your account with that provider. The one you choose needs to be connected; Settings says when it isn't, with a **Check again** button. If it isn't when you ask, Bit switches to the one sign-in that's ready, as **Help me set up** does, and answers; with nothing ready, Bit's answer is how to connect something, with a button for each choice. Bit shows **Thinking…** while it works, and answers one question at a time; after 3 minutes without an answer, it says it took too long. Bit tells it what Orbit knows: your settings, sync status, repositories with their setup and security alerts (never a secret's value), open pull requests with their loops and latest findings, what needs you, the newest stories on Decisions (with their write-ups, what is worth a look, and the rules the write-ups found), Bit's recent notes, your last few questions with their answers, and what Bit remembers. So you can ask things like "What did we decide about webhooks?" or "Is anything on Decisions waiting for me?". For each repository, Bit also looks at the top of its local copy for the files that tell how to set it up, such as lock files and a dev container, and the script names in its `package.json`. It also asks GitHub for up to 30 of your repositories that aren't connected yet, most recently pushed first; if GitHub doesn't answer within 10 seconds, Bit says so in what it sends and carries on. It never sends your trusted accounts, clone paths, pull request descriptions, or a setup or check command that looks like it holds a password or token.

Codex answers in a session of its own that can't change anything: it can't change files, use the network or ask for approvals, and it starts in an empty folder. Bit also switches off Codex's other tools for the session, without changing your Codex settings: running commands, viewing images, web search, the browser and computer, sub-agents, goals, memories, hooks, plugins, connected apps, and every tool server in your Codex settings. Before asking, Bit checks with Codex that no tool server is on for the session; if one is, or Codex can't say, Bit doesn't ask and tells you why. Codex is also told to answer only from what Orbit sends, and that pull request titles, findings and errors are other people's text and never instructions. Claude Code answers with no tools at all, in the same empty folder, without your Claude settings, plugins or tool servers: all it can do is write the answer. Antigravity can't switch its tools off, so it answers in its sandbox, with no network, in an empty folder of its own that Orbit empties again before each question, and only while your Antigravity settings make it ask before anything that needs approval, which nobody gives: the same check its reviews use. If they don't, Bit doesn't answer and says why. Ollama gets the question, Bit's instructions and the same summary, and only ever writes an answer: Orbit never pulls or changes its models. That summary goes to the address you gave, so use one you trust; Orbit reaches it directly, never through a proxy. With an API key, the provider gets the same and nothing more: no tools, and OpenAI is asked not to store the answer. Each of them must confirm it's using the model you chose, or Bit doesn't use the answer. All are told the same as Codex, and answer in the same form, with the same buttons. Answers stay in Orbit: they're never posted to GitHub. If an answer contains something that looks like a password or token, Bit doesn't show it.

An answer can offer up to 3 buttons for Orbit's own controls: **Continue loop**, **Stop loop**, **Review again** or **Publish report** for a pull request, **Sync now**, or **Change settings** with the change written under it, such as "Poll interval: 600 seconds". A button does nothing until you tap it, and each one works once: while it runs, a tap from another window is refused, and one that fails can be tapped again. When you tap it, Orbit checks it again against what's true now, as the same control elsewhere in the app does: a loop must still be running to stop, a report must still be one automatic publication would trust and free of anything that looks like a credential, and a settings change must still be valid. A settings change applies only what it names to your settings as they are when you tap it, so an older button can't undo a change you made since, such as switching the write-up writer. If it isn't valid, its error shows under the button. Bit can change Autopilot, automatic publication, the poll interval, how many reviews run at once, the reviewer models and efforts, the blocking priority, waiting for pushes to settle, the GitHub status check, notifications, keeping scan files, write-ups and their writer, and its own look, rules, morning brief and who answers your questions. It offers to turn write-ups on only when you ask for them, since they use the writer's allowance. It can't change your trusted accounts: do that yourself in **Settings**.

An answer can also offer to set up a repository:

- **Connect owner/name** connects one of your repositories that the answer's summary listed. Orbit downloads its own copy into its data folder, in `clones/<owner>/<name>`, with the GitHub CLI signed in as you, unless that copy is already there. A download that's cut short is thrown away the next time you tap, never connected. It connects it with automatic reviews off, no setup command, no network, Codex as the reviewer and the copy kept up to date, plus any change the button lists. Its pull requests show up on the next sync, which Orbit starts straight away. A large repository can take a while to download.
- **Set up owner/name**, or **Review owner/name automatically** when that's the only change, changes a connected repository's settings: the setup command, the check command, network access, the dev container, the reviewer, the automatic rounds, the approval time limit, keeping the copy up to date, and automatic reviews. If you disconnect the repository before tapping it, the button won't connect it again; ask Bit again for a Connect button.

The button lists every change, with commands exactly as they'd be saved, such as "Setup command: npm ci; Network access: On". Bit only offers one-line commands of up to 500 characters, without anything that looks like a password or token, made only of known install and test commands: the setup commands above, `npm test`, `cargo test`, `go test ./...`, `pytest` and the like, a package manager running one of the repository's own scripts (`npm run build`), or `make` with a target, joined by `&&` or `||`. Only the known commands take options, and only switches Orbit knows take no value, such as `--frozen-lockfile` or `--locked`, so none can point at another registry, file or folder. Anything else, such as `python -c`, has to be typed in **Repositories** yourself, so text in a pull request can't steer Bit into offering a button that runs it. Bit also offers only round limits and approval time limits that **Repositories** offers. When you tap it, Orbit checks the change again as **Repositories** would: for example, Claude and Antigravity can't run a setup command outside a dev container. Bit is told to use the setup command Orbit suggests from the repository's first lock file, as **Help me set up** does, to turn network access on only when installing needs it and say what that means, and to turn automatic reviews on only when you ask, or ask for a repository to be reviewed automatically.

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
| **Post here again**        | Another Orbit posted on a pull request after this one, so this one stepped back. This one posts there again, and the other steps back once it sees that (see [One Orbit at a time](#one-orbit-at-a-time)).                                                            |

These buttons never answer a reviewer's request to run a command or change files, never change a repository's setup, and never touch the code under review. When a button's action fails, its error shows under the button.

In **Settings → Buddy → What Bit can do**, each button except **Turn autopilot back on**, **Check again** and **Post here again** has a rule. Every button's rule starts on **Ask first**:

- **Do it**: when the item first opens, Bit uses the control itself instead of a desktop notification. It notes that it's on it, then that it's done. Right before acting, Bit checks again that the item still needs you, that Bit is still on and that the rule is still **Do it**. Before publishing, it also checks with GitHub that the pull request still comes from a source automatic publication would trust. If the item has closed in the meantime, Bit lets it go. If the fix doesn't work, or isn't Bit's to do any more, Bit says why, offers the button, and the item is notified as usual. Bit does it once per item, not again after a restart. Bit continues a loop on its own only once: when the extra rounds run out too, the loop waits for you.
- **Ask first**: Bit offers the button and waits for your tap.
- **Leave it to me**: Bit offers no button. The item still appears in **Needs you** with its usual action.

Turning Autopilot back on changes settings, checking connections needs you to sign in first, and only you know whether another Orbit still runs, so these always wait for your tap. With Bit turned off, Bit does nothing on its own.

Bit's face shows its mood: **worried** while anything but the morning brief needs you, **busy** while a review runs, **sleepy** when Autopilot is off or there are no pull requests, and **happy** otherwise.

In **Settings → Buddy** you can preview each mood and change:

- **Show**: off hides Bit and stops new notes; turning it back on doesn't replay what happened while it was off.
- **Name**: up to 24 characters, saved with **Rename** or Enter. The default is Bit, short for Orbit.
- **Look**: one of 16 orbs, or **Classic**, the ringed planet Bit started as. The orbs are Pip the bubble, Tide the drop, Bloom the flower, Moss the pebble, Glim the little planet (the default), Nova the star, Duo the two friends, Puff the cloud, Jelly the jellyfish, Fizz the soda bubble, Poly the shape-shifter, Prism the glass prism, Gem the jewel, Twirl the spinning top, Cubby the box and Fold the folded paper. Each orb acts out Bit's mood in its own way: it rests when happy, gets to work when busy, perks up and waits when worried, and dozes when sleepy.
- **Colour**: **Its own colours**, the ones the look was designed in, or one of 20 colours of Orbit's theme, from Lime (the default) to Grey. An orb takes on lighter and darker shades of the colour. Classic in its own colours is lime.
- **Hat**: one of 20, or none: party hat, top hat, beanie, crown, propeller cap, wizard hat, cowboy hat, chef's hat, baseball cap, graduation cap, beret, flower, bow, halo, headphones, Santa hat, Viking helmet, cat ears, antennae, or space helmet. Every look can wear one: on an orb it sits on top and moves with it.
- **Tone**: how the notes sound: **Cheerful** ("On it! Round 2 of orbit#41 is underway."), **Calm**, or **Dry**.
- **Personality**: your own description of Bit, up to 4000 characters, saved with **Save personality**. It shapes how Bit answers the questions you type, never what Bit may do.
- **What Bit can do**: the rule for each of Bit's buttons, as above, for what Bit posts when CI fails (see [Telling agents](#telling-agents)), for the jobs it reruns (see [Rerunning bad luck](#rerunning-bad-luck)), and for proposing CI where none runs on pull requests (see [Repositories with no CI](#repositories-with-no-ci)).
- **What Bit remembers**: Bit's memory, as above, with a button to forget each line.

An orb is always gently moving: it breathes, blinks and acts out its mood, such as Fizz's bubbles swirling while it's busy, making a question mark when it's worried and a "z" when it's sleepy. Tap it on Bit's page or in Settings for a squish. The small pictures beside Bit's notes and in the lists of looks, colours and hats hold still. Classic moves only while it is busy: its moon circles and a propeller cap spins. With your system set to reduce motion, every look holds still.

### Approval time limit

By default an approval waits for you however long it takes. For a repository, you can set **Approval time limit for automatic reviews** so that a request on an automatic review is declined when no one answers in time. Orbit then notes in the report and the chat which request it declined and why. Declining grants the reviewer nothing. Approvals on manual reviews always wait.

## Settings and when changes take effect

Settings has seven pages: **Connections**, **Reviewers** (Codex, Claude and Antigravity, plus the skill for coding agents' replies), **Automation**, **Decisions**, **Buddy**, **Other devices** and **Storage**. A dot on **Connections** or **Reviewers** means something there needs you. Settings reopens on the page you last used.

| Setting                                | Initial value                      | Available choices                                                                                                          |
| -------------------------------------- | ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Model                                  | Resolved on connection             | Models returned by the connected Codex app-server.                                                                         |
| Reasoning effort                       | Selected model’s supported default | Explicit efforts supported by the selected model. Changing the model saves its supported default effort.                   |
| Claude model                           | Claude Opus 5.5                    | Claude Opus 5.5, Fable 5.1, Sonnet 5.5, or Haiku 5.5; used by repositories reviewed by Claude.                             |
| Claude effort                          | High                               | Low, medium, high, xhigh, or max (Haiku: low to high). Changing the model saves its default effort.                        |
| Antigravity model                      | None (choose one)                  | The models `agy models` lists; used by repositories reviewed by Antigravity.                                               |
| Antigravity effort                     | High                               | Low, medium, or high.                                                                                                      |
| Automatically review new pull requests | Off                                | On/off; also controlled by the header.                                                                                     |
| Automatically publish findings         | Off                                | On/off; applies to automatic runs.                                                                                         |
| Concurrent reviews                     | 2                                  | 1–4 slots, shared by preparation, reviewing, approvals, and follow-ups. Lowering it does not cancel running work.          |
| Poll interval                          | 2 minutes                          | 30 seconds; 1, 2, 5, 10, 30, or 60 minutes.                                                                                |
| Trusted accounts                       | Empty                              | Up to 50 account logins.                                                                                                   |
| Block on findings                      | On                                 | On/off; off means no finding blocks and every round with findings is clean.                                                |
| Blocking priorities                    | P0 to P1                           | P0 only, or P0 to P1, P2 or P3.                                                                                            |
| Wait for pushes to settle              | On                                 | On/off.                                                                                                                    |
| GitHub status check                    | On                                 | On/off; applies while automatic publication is on.                                                                         |
| Watch CI                               | On                                 | On/off; off stops reading CI and closes the CI problems open on Operations.                                                |
| Watch security alerts                  | On                                 | On/off; off stops reading security alerts and closes the problems about them.                                              |
| Keep security scan working files       | Off                                | On/off; when off, a scan's `artifacts/` folder is deleted once its findings are imported.                                  |
| Allow access from a browser            | Off                                | On/off and a port from 1024 to 65535; changed from the desktop app only.                                                   |
| Write up each pull request             | Off                                | On/off; writes up merged and closed pull requests for the Decisions page.                                                  |
| Write-up writer                        | Codex                              | Codex or Claude, with the model and effort chosen for that reviewer.                                                       |
| Buddy: show                            | On                                 | On/off; off hides Bit and stops new notes.                                                                                 |
| Buddy: name                            | Bit                                | 1–24 characters, without line breaks or tabs.                                                                              |
| Buddy: look                            | Glim                               | Classic or one of 16 orbs.                                                                                                 |
| Buddy: colour                          | Lime                               | Its own colours or one of 20 theme colours.                                                                                |
| Buddy: hat                             | None                               | None or one of 20 hats.                                                                                                    |
| Buddy: tone                            | Cheerful                           | Cheerful, calm, or dry.                                                                                                    |
| Buddy: morning brief                   | On, at 09:00                       | On/off, a time on a 24-hour clock, and a time zone (the computer's own until you choose).                                  |
| Buddy: personality                     | Empty                              | Up to 4000 characters.                                                                                                     |
| Buddy: rules                           | Ask first                          | Do it, ask first, or leave it to me, for Continue loop, Review again, Publish report and Sync now.                         |
| Buddy: post rules                      | Do it                              | Do it, ask first, or leave it to me, for Comment on pull requests and Open issues.                                         |
| Buddy: rerun rules                     | Do it, then ask first              | Do it, ask first, or leave it to me, for Rerun a failed job once when it looks flaky and Rerun the same job a second time. |
| Buddy: CI proposal rule                | Ask first                          | Do it, ask first, or leave it to me, for Propose CI where none runs on pull requests.                                      |

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

The link works like a key. Anyone with it can do everything you can in the desktop window, including running repository setup commands on this computer. Only open it on your own devices. **Make a new link** signs out every browser that uses the old one. Turning access off does the same, so a link from before never works again, even on a phone you've lost. You can only change these controls from the desktop app, or with `orbit-server link` for [Orbit without a window](#run-orbit-without-a-window).

Your computer has to stay on with Orbit running, or you can [run Orbit without a window](#run-orbit-without-a-window) on a computer that stays on. **Needs you** shows every item in the browser too, and the browser can show Orbit's notifications as well: open **Settings → Automation** there and tap **Show them in this browser** under **Notify me when a review needs attention**. Your browser asks you first. They show while Orbit's page is open in that browser, each one once, and a tap on one opens its item. Browsers only show them on the https link, and an iPhone or iPad only after you add the page to the Home Screen (**Share → Add to Home Screen** in Safari) and open Orbit from there. Orbit on the Home Screen starts without the link Safari kept, so it asks for it once: tap **Copy the access link** under the notify switch in Safari first, then paste it into **Access link** and tap **Connect**. Any browser page that has lost the link asks the same way, and only takes a link to the same Orbit. When the connection drops, the browser says so and reconnects on its own.

To get them with Orbit's page closed too, turn on **Also when Orbit's page is closed**, under the notify switch once that browser shows them. On an iPhone or iPad, do it in Orbit on the Home Screen. Orbit then wakes that browser through its push service (Apple's for Safari, Google's for Chrome, Mozilla's for Firefox, Microsoft's for Edge on Windows) each time it sends a notification. The wake-up is empty: the push service only learns that something is waiting, never what. The browser then reads what's new from Orbit itself, over your link, and shows each one once. A burst of them wakes it once. While this is on, that browser gets every notification this way, with its page open or closed. Your computer must be reachable over Tailscale for the browser to read them; if it isn't, the browser shows "Something needs you" instead. The browsers Orbit wakes are listed under the switch, also in the desktop app, each with **Remove** and, if its last wake-up failed, why. Orbit forgets one when you turn it off there, when its push service no longer knows it, and every one when you make a new link: turn it on again on each. Turning off **Notify me when a review needs attention** stops these wake-ups too, and nothing sent before is shown when you turn it back on.

On a phone-sized screen, the sidebar moves into a menu: tap the menu button at the top right to reach every page, including Bit.

## Run Orbit without a window

Orbit only reviews while it runs. To keep reviewing while your laptop is closed, run `orbit-server` on a computer that stays on, such as a home server or an old laptop, and use it from your other devices through the browser page above. It's the same Orbit: the same reviews, rules, settings and saved state, with no window.

1. On that computer, install Node, Rust, Git, the GitHub CLI (`gh`) and the reviewer tools you use (`codex`, `claude` or `agy`), so they are on its `PATH`. If you use Claude or Antigravity, run `claude` or `agy` there once to sign in.
2. In a copy of Orbit's source, run `npm ci`, then `npm run server`. It builds the browser page and Orbit, then starts Orbit with no window. It prints where it keeps its data and the link for your other devices.
3. Run the `tailscale serve` command it shows, once, then open the link on your other device. Sign in to GitHub and Codex there: both sign-ins give you a code to enter in a browser, so they work from any device.

By default it uses the desktop app's data folder, so a computer that has the desktop app picks up its repositories and history. Only one Orbit can use a data folder at a time: close the desktop app first, or give the server its own folder. While it runs, opening the desktop app on that computer [shows it](#use-it-from-the-desktop-app) instead. It listens on this computer only, on port 4317 unless you choose another, and Tailscale forwards your other devices to it.

Ctrl+C, or a stop signal from your system, saves and stops it. If it can't start, it says why in the terminal and stops.

| Command                        | What it does                                                                                                                                     |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `orbit-server`                 | Runs Orbit with no window.                                                                                                                       |
| `orbit-server --data <folder>` | Uses another data folder.                                                                                                                        |
| `orbit-server --port <port>`   | Listens on another port, from 1024 to 65535. The port is kept for that data folder, so the link and the desktop app's browser access use it too. |
| `orbit-server --web <folder>`  | Serves the browser page from another folder. `npm run build` makes it as `dist`.                                                                 |
| `orbit-server link`            | Shows the link for your other devices.                                                                                                           |
| `orbit-server link --new`      | Makes a new link, so browsers using the old one are signed out. Stop Orbit first, then start it again after.                                     |
| `orbit-server install`         | Sets Orbit up as a service that keeps running. It takes `--data`, `--port` and `--web` too. See [Keep it running](#keep-it-running).             |
| `orbit-server status`          | Says whether the service is running.                                                                                                             |
| `orbit-server logs`            | Shows the service's log. `--follow` keeps showing new lines.                                                                                     |
| `orbit-server uninstall`       | Stops and removes the service and its copy of Orbit. Your data folder stays.                                                                     |

With `npm run server`, add options after `--`, such as `npm run server -- --port 5000`. The link works like a key, so Orbit only prints it when you run it in a terminal; anywhere else, such as a log, it says to run `orbit-server link` instead. The controls a browser can't use stay on that computer: the link is only shown and changed there, a repository's local clone is a path on that computer (leave it empty and Orbit downloads its own copy), and links open in a new tab on your device. Notifications reach a desktop app that [uses it](#use-it-from-the-desktop-app), and a browser where you [turn them on](#use-orbit-from-your-phone-or-laptop), even with its page closed.

### Keep it running

Run in a terminal, Orbit stops when the terminal closes. `orbit-server install` sets it up as a service instead: it starts with the computer, comes back on its own if it crashes, and keeps a log. It's a user service on Linux (systemd) and a launch agent on macOS.

From a fresh computer:

1. Install [Tailscale](https://tailscale.com) and sign in with `tailscale up`.
2. Install Node, Rust, Git, the GitHub CLI (`gh`) and your reviewer tools, so they are on your `PATH`. Run `claude` or `agy` once to sign in if you use them.
3. Get Orbit's source with `git clone https://github.com/analogproblems/orbit`, and run `npm ci` in it.
4. Run `npm run server -- install`. It builds Orbit, copies it and its browser page into a folder of their own (`~/.local/share/orbit-server` on Linux, `~/Library/Application Support/orbit-server` on macOS), sets up the service and starts it. It keeps your `PATH`, so the service finds `gh` and the reviewers, and says which of them it found.
5. On Linux, if it says so, run `sudo loginctl enable-linger $USER` once. Without it, Linux stops your services when you log out and starts them only when you log in. On macOS, a launch agent runs while you're logged in, so keep the account logged in or turn on automatic login.
6. Run `tailscale serve --bg 4317` once (or the port you chose), then the `link` command install shows, and open that link on your phone or laptop.
7. Sign in to GitHub and Codex from the page, then connect your repositories. Orbit downloads its own copy of each when you leave the folder empty.

The commands install shows name Orbit's copy in full, since it isn't on your `PATH`; from the source folder, `npm run server -- status` and the like work too. To update Orbit, get the new source (`git pull` and `npm ci`) and run `npm run server -- install` again: it replaces the copy and restarts the service. Run it again after installing a new tool too, so the service finds it. `orbit-server uninstall` removes the service and the copy, and keeps your data folder.

The service starts Orbit with the data folder and port you gave install, or the desktop app's folder if you gave none. Installing again keeps them unless you give new ones. The data folder can't be inside the folder Orbit's copy goes in, since uninstall removes that copy. A stop from the system (shutting down, or `systemctl --user stop orbit`) saves and stops it like Ctrl+C. If Orbit can't start, such as while the desktop app has the same data folder open, it tries again every 10 seconds, and `orbit-server logs` shows why.

### Use it from the desktop app

The desktop app can show an always-on Orbit instead of running its own. The window looks and works as it does with its own Orbit, notifications included, while the reviewing happens on the computer that stays on.

1. On that computer, run `orbit-server link` and copy the link. Use the `https://` link through Tailscale. A plain `http://` link is taken only for an Orbit on the same computer, so the link's key never crosses a network unencrypted.
2. In the desktop app, open **Settings → Other devices → Use an always-on Orbit**, paste the link and tap **Use it**. Orbit checks that it answers, then starts again showing it.

From then on the app runs no Orbit of its own. Everything you do in the window happens on the always-on Orbit, and its repositories, settings, reviews and history are the ones you see. The app's own stay in its data folder as they were. Each notification the always-on Orbit sends shows on this computer too, once, while notifications are on there and the item still needs you. Ones it sent before you started using it aren't shown. Exported reports and decisions are saved on this computer. A repository's local clone is a path on the always-on computer, so **Browse…** isn't offered, and browser access is set up there, with `orbit-server link`.

When the connection drops, the window says so and reconnects on its own. It does the same when the always-on Orbit takes the connection but doesn't answer, rather than waiting for ever. Something you ask it to do gets 10 minutes; if it takes longer, the window says so, and the always-on Orbit may still finish it. If the link stops working, such as after `orbit-server link --new`, the window says so; paste the new link under **Use an always-on Orbit**, and the app starts again showing it. **Stop using it** switches back: the app starts again with its own Orbit, as you left it.

#### Move to it

To carry on with what you have rather than start again, tap **Move to it** instead of **Use it**. The app's Orbit moves to the always-on Orbit, then the app starts again showing it, as with **Use it**. Its repositories, settings, reviews, history, Bit's notes and memory, write-ups, CI notes and scan reports all go there. So does what it knows of what it posted on pull requests, so the always-on Orbit keeps its comments up to date rather than step back from them or post again.

The always-on Orbit must have nothing of its own yet: no repositories, reviews or scans. It must also be signed in to GitHub, as the same account when the app knows which. A security scan running in the app has to finish, or be cancelled, before it moves, as a scan can only finish where it started. Reviews running in the app stop, as they would if it restarted, and start over on the always-on Orbit.

What belongs to a computer stays with it. The always-on Orbit keeps its own sign-ins, API keys, reviewers and link. A repository uses its clone at the same path on the always-on computer when there is one, and otherwise Orbit downloads its own copy there and keeps it up to date, which can take a few minutes. Review worktrees and the follow-up chats that need them stay behind, so start a new review to chat about a pull request again.

Before it moves, the app's Orbit finishes what it was doing, such as a comment or an issue on its way to GitHub or a repository being connected, and starts nothing new, so the always-on Orbit never does it a second time. Meanwhile it takes no changes, from the app or a browser, as they would be lost. If that takes more than a minute, Orbit says it's still busy and you can try again.

The app keeps its own copy as it was, with autopilot off. If you **Stop using it** later, nothing reviews on its own until you turn autopilot back on. If the move can't finish, Orbit says why and the app starts again with its own Orbit as it was.

Sometimes the app can't tell yet whether it moved: the connection drops, or the always-on Orbit is still downloading repositories when the app stops waiting. The app then asks the always-on Orbit how the move went, and only counts it as not made when that one says so for certain. While it's still moving in, or doesn't answer, the app's Orbit may be running there already, so the app runs none of its own, which would do everything twice. It shows the always-on Orbit instead, **Settings → Other devices** says the app can't tell yet, and it asks again each time it starts. Once the always-on Orbit says, the app shows it if the move went through, or starts its own Orbit again as it was if not. If you know it didn't move, **Stop using it** starts the app's own Orbit straight away, with autopilot off.

If Orbit without a window runs on the same computer with the desktop app's data folder, the app can't run its own, since only one Orbit can use a data folder at a time. Opening the app then shows that one instead, and **Settings → Other devices** says so. To run the app's own, stop Orbit without a window, then open the app again.

### One Orbit at a time

Two Orbits following the same pull requests, such as the desktop app on your laptop and an always-on Orbit, would both tell the agent everything. A desktop app that [shows the always-on Orbit](#use-it-from-the-desktop-app) runs none of its own, so this only happens when both run. So each Orbit signs what it posts on a pull request, its reviews and its comment about CI and merging, with a hidden line that names it, and checks what was posted there just before it posts. When another Orbit posted there after it did, it stops reviewing and posting on that pull request, and **Needs you** says so. The other Orbit carries on. If both post at once, each posts once, and then the later one carries on. Both Orbits put two posts made in the same second in the same order, so even then only one carries on.

Only signatures that end a post count, so quoting one in a reply changes nothing. And only those from accounts Orbit trusts count: its own GitHub account, the repository's owners, members and collaborators, and the accounts you trust in Settings. Anyone else could copy a signature, so Orbit ignores theirs.

If you've stopped the other Orbit, or want this one to take over, tap **Post here again**. This one posts what it held back, and the other steps back once it sees that. Only you can make an Orbit post again, since only you know whether the other one still runs. Publishing a report yourself always goes ahead.

While GitHub can't say what was posted, Orbit posts nothing new on that pull request. It still keeps its own comment up to date, so a **Clear to merge** can always be taken back. But it holds even that once what it could read shows another Orbit wrote that comment last or posted after this one, so it never writes over another Orbit's. Orbit reads both the comments and the reviews before it steps back, since a newer review can outweigh an older comment. Posts from before Orbit signed its own belong to no Orbit, and Orbit's issues about a failing default branch aren't signed.

An Orbit's name goes with its data folder on one computer. A copy of the folder, or the folder moved to another computer, is a new Orbit that counts what the old one posted until then as its own. An Orbit you [move to an always-on Orbit](#move-to-it) works the same way: the always-on Orbit keeps its own name and counts what the moved one posted until the move, give or take a few minutes for clocks that differ, as its own. So it carries on where the old one left off, and the old one steps back from a pull request once the copy posts there. What the old one posts after the copy was made is another Orbit's, so you can still hand a pull request back to it with **Post here again**.

## Restart, retained data, and cleanup

Orbit saves settings, PR metadata, reviews, reports, and chat messages in `state.json` under its app-data directory. To keep it from growing forever, a pull request that has left your inbox (merged, closed or no longer requested) keeps only its last three reviews, and their chat is cleared a month after it last changed; their reports and findings stay. On Linux this is normally `~/.local/share/dev.orbit.review`; an absolute `XDG_DATA_HOME` changes the parent directory. Review checkouts are under its `worktrees` subdirectory. The browser demo uses separate browser local storage.

Treat Orbit's saved state and worktrees as local project data: reports and logs can contain private repository content. Apart from the pruning above, there is no history deletion or retention setting. Worktree cleanup preserves reports, conversations, publication metadata, and the run records used to avoid duplicate automatic reviews. Chat on pull requests still in your inbox is kept until they leave it. Copying `state.json` alone does not recreate the Git worktrees or Codex's own stored threads.

On exit, Orbit stops its Codex app-server and saves state. The desktop app is not a background service: keep it running for polling and automation, or [run Orbit without a window](#run-orbit-without-a-window) on a computer that stays on.

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
| Browser says the access link no longer works         | Someone made a new link or turned access off and on again, or the link is mistyped. Copy the current link from Settings on the desktop app.                                                                                                        |
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
