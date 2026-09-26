# Orbit user guide

Verified against the application code on September 23, 2026, including the usability and storage changes on `codex/usability-storage` based on commit `7768c85433e71946bf2163606a53e018fb0d8e3d`. This guide describes the desktop app. The browser demo has simulated behavior and separate data.

Orbit connects open GitHub pull requests to local Codex review runs. Each run has a Git worktree and a chat. You can start reviews yourself, let Autopilot start eligible reviews, and publish completed reports as PR comments.

## Your first review

1. Launch the desktop app and open **Settings**. Check that **Codex CLI** and **GitHub CLI** show **Connected** and that GitHub shows the account you intend to use.
2. Open **Repositories → Add repository**. Choose a GitHub repository and enter the absolute path to its existing local clone. Leave automatic reviews off for a manual first run, then click **Save repository**.
3. Open **Pull requests**. Saving the repository starts a sync; the refresh button also syncs on demand.
4. Select an open PR and click **Start review**. Open **Review chat** to follow its progress. Expand **Execution log** entries to inspect command and setup output.
5. If a banner says approval is needed, open **Review request**, inspect the requested action, and choose **Approve once** or **Deny**.
6. When the status becomes **Ready to publish**, read the report and the findings list under it. You can ask a follow-up before publishing.
7. Choose **Publish findings** in the chat or **Publish review** in the footer. Inspect the preview, then click **Publish comment** to post using your connected GitHub account.

Starting a review manually does not publish its report automatically, even when automatic publication is enabled.

After the report arrives, the chat shows **Extracting findings…** while Codex restates its findings as a list, on the same chat and with the same pinned model and effort. The list shows each finding's severity, file and lines, with the verdict and any tests Codex could not run. If this step fails, the run is labeled **Unstructured run**: its report is kept and can still be published, and exports include the verdict and a findings table when there is one. Stopping the run during this step stops it like any other review.

When you review a new head of a PR that Orbit already reviewed with a findings list, the review starts from that earlier round. Codex is given the earlier findings, the commits and the size of the changes since, and comments posted on the PR since the earlier report, but only from the PR author (when Orbit trusts the PR), your own account and your trusted accounts. Comments from anyone else are left out and counted, and the chat says how many. Codex still reviews the whole change against the base, marks each earlier finding as fixed, still open, declined (accepted) or declined (disputed), and reports only new problems as findings. The review chat and exports show this under **Since the last round**. After a rebase or force-push the earlier findings are still checked, but the commits since are not listed.

## Launch and connect

On this Linux checkout, launch the compiled app with:

```sh
/home/jdub/Documents/ChatGPT/Orbit/src-tauri/target/release/orbit
```

Close an existing Orbit instance before launching another. Orbit allows only one instance using the same app-data directory.

The desktop app runs the `gh`, `git`, and `codex` programs from its environment. It uses the existing GitHub CLI authentication and Codex account configuration; there is no in-app login form or token field. If a connection is missing, use the relevant login command in a terminal:

```sh
gh auth login
codex login
```

Then use **Settings → Reconnect**. That action connects Codex and, if the connection succeeds, syncs GitHub. The inbox refresh button can sync GitHub separately. If launching through a desktop shortcut cannot find the CLIs, launch the executable from the terminal where they work.

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

In **Repositories**, click **Add repository** or **Connect a repository**.

| Field or control                 | How to use it                                                                                                                |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| **GitHub repository**            | Search repository names or descriptions, then select a result. Private/public labels describe GitHub visibility.             |
| **Refresh repositories**         | Reload the picker after gaining access to a repository or creating one. The list otherwise stays cached for the session.     |
| **Use owner/name**               | Type a complete repository name to connect one not shown in the picker. This does not grant access.                          |
| **Local checkout path**          | Enter an absolute path to an existing clone. Orbit does not clone it for you. A literal `~` is not an absolute path.         |
| **Environment setup**            | Optional shell command to prepare each new review worktree. Leave blank when host tools and checked-in files are sufficient. |
| **Allow sandbox network access** | Allows network access for setup and the new review's agent environment. Off by default.                                      |
| **Include in automatic reviews** | Makes this repository eligible when workspace Autopilot is also on. Off by default.                                          |

Click **Save repository**. Saving a new repository or changing its name/path verifies an existing Git working tree and a matching supported GitHub `origin`, then stores the canonical checkout root. Validation does not fetch or run setup. Errors stay in the dialog so you can correct the configuration. Saving starts an inbox sync. Review preparation checks the checkout again because it may have changed. An unavailable existing checkout does not prevent disabling automation, disconnecting it, or changing unrelated Settings.

Filesystem validation runs outside the async workers, with a ten-second wait limit per filesystem check. If a mount stalls, validation reports a timeout and leaves the configuration unchanged. The operating system may finish the underlying filesystem request later; its late result does not save the repository.

The picker loads up to 1,000 accessible repository entries and omits archived repositories. It displays up to 50 unconnected search matches at a time and shows the total when more matches exist; narrow the search or enter `owner/name` directly. Repository names are treated case-insensitively and cannot be connected twice.

The clone's `origin` must match the configured GitHub repository. Accepted forms include `https://github.com/owner/name`, `git@github.com:owner/name`, and `ssh://git@github.com/owner/name`, with an optional `.git` suffix. Custom SSH host aliases do not match these forms.

### Set up the review environment

Orbit creates a detached worktree at the PR's captured head commit. It uses host-installed runtimes and tools; it does not automatically build a container, interpret a devcontainer configuration, or copy the original clone's untracked dependencies into the worktree.

The setup command runs inside that new worktree through the Codex sandbox, using `/bin/sh` on Unix or PowerShell on Windows. It has a five-minute execution timeout. Its output is available in the chat's execution logs; a nonzero exit stops review preparation.

For a compatible npm project, one possible setup command is:

```sh
npm ci --ignore-scripts --cache .cache/npm
```

Choose a command appropriate to the repository. Dependency downloads need sandbox network access. Install scripts can execute PR code; `--ignore-scripts` avoids those scripts but may leave some projects needing additional setup.

The network switch controls the review sandbox, not Orbit's GitHub requests, Git fetches, or Codex's service connection. The worktree and sandbox are not a complete read-isolation boundary. The agent is instructed to review without editing tracked source, but this instruction is not a read-only filesystem guarantee.

Use the configure icon on a repository card to edit its settings. Changes do not rebuild existing review worktrees; start a new run when you need a new environment. **Disconnect** removes the configuration and starts a sync. It does not delete the original clone or the saved chat history. Finished review worktrees become eligible for cleanup after the repository leaves the inbox; use **Stop** separately for running work.

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

Use **Stop** in the PR detail or Review activity to cancel queued work or interrupt active work. Publishing is a separate state and does not have a Stop control.

**Review activity** lists runs, including those whose PR has left the inbox. **In progress** includes queued runs as well as executing runs. Open a row to read its chat. When a PR has multiple runs, the **Review history** dropdown in its chat selects one. The sidebar shows up to five recent runs. **Ready to publish** counts use the same rules as the badges and publication buttons. Derived labels do not overwrite stored review outcomes; a published report retains **Published** even when the PR changes.

The chat shows short status notes inline. Expand **Execution log** entries for multiline setup output and recorded command output. Some output is capped, so these logs are not a complete terminal transcript. The composer stays docked at the bottom; a **Scroll to bottom** button appears when you scroll away from the latest messages.

In long inbox and activity lists, the first ten visible entries use live relative timestamps. The ten latest chat entries also use live relative timestamps; older entries show an absolute date and time to avoid a timer for every retained message.

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

Publishing creates a regular comment in the PR conversation using the connected GitHub account. It does not submit an Approve/Request changes review, create inline review comments, or merge the PR.

The publication dialog previews the stored report. A failed request leaves the preview open with its error and **Retry publication**. A successful request closes the preview. Published run details, chats, and Activity offer **View comment on GitHub** when the saved comment URL is available. The posted comment also includes an Orbit heading, the reviewed head and base commit IDs, and model/effort labels. The preview is not an editor, and there is no report-editing or comment-updating control.

Before posting a new comment, Orbit requires a completed, nonempty report and rechecks that the PR is open and its head and base still match. If new commits are available, sync and use **Review again**. A GitHub change can still occur between the final check and the write; the comment identifies the exact commits reviewed.

Reports matching recognized credential formats are held from automatic publication. Manual publication requires the **I checked the report** switch before **Publish comment** becomes available. The formats include common service tokens, private-key headers, and JSON web tokens. This check does not detect every secret or redact matching text. Inspect the report before overriding it.

On an uncertain posting result, retry publication of the **same run**. Orbit looks for that run's existing comment marker before posting again. This avoids starting a fresh review merely to retry a comment. It can also confirm a comment that was posted before the PR subsequently changed. A formatted comment over Orbit's 60,000-byte threshold is refused; the error asks you to copy the report from the chat instead.

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
- GitHub identifies the author as an owner, member, or collaborator, or the login is in **Trusted accounts**.
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

Each automatic review of a PR is one round of its review loop. Orbit gives every round with a findings list its own verdict: **changes requested** when a finding at or above the **Blocking findings** priority is new or was found still open or disputed, and **clean** otherwise. Codex's own verdict is shown beside it when the two differ. The verdict is recorded when the findings are extracted, so changing the setting affects later rounds only.

The PR detail and inbox show the loop's state: **Reviewing**, **Waiting for author**, **Clean**, **Needs you** with the reason, or **Stopped**. A loop needs you when a review failed or has no findings list, an approval is pending, a report is waiting for you to publish it, the reviewer disputes a finding the author declined, the same blocking findings stayed open across two rounds, or the round limit is reached. Each repository allows 4 automatic rounds per PR by default (**Automatic rounds per pull request**). **Continue loop** allows two more; **Stop loop** pauses automatic reviews of that PR. **Review again** always works and does not count as a round.

When a PR gets new commits while an automatic round is queued or running for the older head, that round is marked **Superseded**: a queued one is cancelled, a running one finishes but is not published or counted. With **Wait for pushes to settle** on, Orbit starts the next round only after the head has stayed the same for one poll interval, so a burst of pushes gets one review. This also delays the first review of a newly found PR by one interval.

While automatic publication is on, Orbit also shows each automatic round's result on the reviewed commit as the **orbit/review** status check: pending while the round is in progress, success when clean, failure when changes are requested, and error when the loop needs you. Its description gives the round and the blocking count, such as "Round 2 · 1 blocking finding". Published comments also carry a hidden line with the verdict for agents to read, such as `<!-- orbit-verdict: changes_requested round=2 blocking=1 -->`. Turn off **GitHub status check** in Settings to stop setting the check. If GitHub refuses a status, the PR detail says why; the review is not affected. Orbit never makes the check required: that is a branch protection choice in GitHub.

## Settings and when changes take effect

| Setting                                | Initial value                      | Available choices                                                                                                 |
| -------------------------------------- | ---------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Model                                  | Resolved on connection             | Models returned by the connected Codex app-server.                                                                |
| Reasoning effort                       | Selected model’s supported default | Explicit efforts supported by the selected model. Changing the model saves its supported default effort.          |
| Automatically review new pull requests | Off                                | On/off; also controlled by the header.                                                                            |
| Automatically publish findings         | Off                                | On/off; applies to automatic runs.                                                                                |
| Concurrent reviews                     | 2                                  | 1–4 slots, shared by preparation, reviewing, approvals, and follow-ups. Lowering it does not cancel running work. |
| Poll interval                          | 2 minutes                          | 30 seconds; 1, 2, 5, 10, 30, or 60 minutes.                                                                       |
| Trusted accounts                       | Empty                              | Up to 50 account logins.                                                                                          |
| Blocking findings                      | P0 to P1                           | P0 only, or P0 to P1, P2 or P3.                                                                                   |
| Wait for pushes to settle              | On                                 | On/off.                                                                                                           |
| GitHub status check                    | On                                 | On/off; applies while automatic publication is on.                                                                |

Settings switches and selections save immediately. Changing the model saves that model and its explicit supported default effort in the same update. **Choose a model** and **Choose an effort** are placeholders, not executable default choices. **Saving…**, **Saved**, or an error with **Retry saving** shows the result. **Saved** clears after 2.5 seconds; errors remain until resolved. After a failed write, controls return to the active values. The unapplied request is listed beside its error and **Retry saving**, and survives page navigation along with typed account input. Retry after resolving the failure. If a model change fails, effort stays disabled until you retry successfully or choose a model again; this prevents a later retry from resetting a newly selected effort.

The header Autopilot switch, repository-row automation switches, and PR **Trust** button also save immediately. Updates change only their intended fields. Global Settings changes are ordered separately from repository changes, so slow checkout validation does not delay turning Autopilot off. Repository dialogs retain **Save repository** because their fields form one configuration that must be validated together.

On connection, Orbit reads your effective Codex configuration when either workspace value is empty. An empty model resolves to the configured model if it appears in the catalog, otherwise to the catalog's unique default model. An empty effort uses your configured `model_reasoning_effort` when the selected model supports it, otherwise that model's supported default effort. This also applies when you already have an explicit model and only effort is missing. If only effort is missing and Codex's configuration cannot be read, Orbit uses the supported model default. If the model is also missing, a failed config read blocks resolution. Missing or unsupported fallback defaults block resolution too.

Orbit saves both values atomically and displays a notice identifying where each value came from: your saved Orbit selection, your Codex configuration, or a model/catalog default. The notice explains fallbacks, including an unsupported configured effort or unreadable configuration, and clears after the next successful connection. There is no hard-coded latest model. The browser demo mirrors this with a simulated Codex configuration.

Orbit never replaces a nonempty explicit selection automatically. A missing model, unsupported effort, ambiguous catalog default, discovery failure, or failed save blocks new reviews with an explanation under **Review agent**. Codex can show **Connected** while review settings still need attention. **Reconnect** refreshes the model catalog and retries resolution even if the app-server is already connected. The last verified catalog remains usable during the refresh; an actual discovery failure blocks execution until a successful refresh. Choose valid settings to correct an unavailable selection. Unrelated settings, repository controls, and switching automation off remain available.

Orbit requests explicit settings and rejects mismatched responses instead of asking for provider model fallback. It does not edit your Codex configuration. The integration is based on Codex CLI **0.156.1** and the [official app-server documentation](https://developers.openai.com/codex/app-server). The browser demo uses a simulated explicit model and the same pinning and legacy-selector workflow; it does not discover your account's models.

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

For an inactive run with a nonempty report, choose **Export report** in its chat or Review activity. The desktop app opens a native **Save As** dialog. Choose a Markdown destination or cancel normally. If you omit the extension, the dialog opens again with `.md` added for you to confirm the final filename and any overwrite. An explicitly entered extension is kept. Orbit writes only the final dialog-selected destination. A write failure is shown next to the action and can be retried.

The Markdown contains the stored report, PR identity/link, reviewed head and base commits, creation time, saved model/effort labels, and publication link when available. It excludes chat transcripts and execution logs. Export does not post to GitHub. In the browser demo, export downloads Markdown; storage sizes and cleanup are simulated.

## Troubleshooting

| What you see                                         | What to check or do                                                                                                                                                     |
| ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Sample PRs or **Interactive demo**                   | Launch the native executable or `npm run desktop`. The browser session has no real workspace connection.                                                                |
| Empty inbox                                          | Connect a repository, clear the visible filters, confirm it has open PRs, then refresh. Check Settings for a GitHub sync error.                                         |
| Repository missing from the picker                   | Refresh the picker, narrow the search, or type its `owner/name`. Confirm the connected account can access it.                                                           |
| **Cannot start gh/git/codex**                        | Make the named program available in the app's launch environment. Launch from a terminal where it works.                                                                |
| Codex not signed in or disconnected                  | Complete `codex login` in a terminal and use **Settings → Reconnect**. Failed initial reviews need a new run.                                                           |
| GitHub sync needs attention                          | Read the GitHub error in Settings; check CLI authentication and repository access, then refresh. Cached entries may still be visible.                                   |
| Checkout missing or origin mismatch                  | Configure the correct absolute clone path and matching GitHub `origin`, then start a new review.                                                                        |
| Environment setup failed                             | Expand **Execution log** entries, inspect setup output, correct the command or dependency/network configuration, and use **Review again**.                              |
| PR changed while preparing, or new commits available | Refresh the inbox and start a new review. Old reports cannot be newly posted against changed commits.                                                                   |
| Autopilot appears idle                               | Check both review switches, Codex connection, successful repository sync, draft/fork/author eligibility, previous runs for this revision, and occupied slots.           |
| **All review slots are in use**                      | Finish or stop an active run, resolve approvals, or increase concurrency before sending a follow-up.                                                                    |
| Review stays at **Needs approval**                   | Open the approval banner and decide. Dismissing the dialog does not resolve the request.                                                                                |
| Settings changes show a save error                   | Resolve the local write or validation error, then choose **Retry saving**. A failed change is not active.                                                               |
| Files failed to load                                 | Use **Retry** in **Files changed**.                                                                                                                                     |
| Cleanup failed                                       | Check the per-run error under Storage, correct the cause, refresh, and select the run again.                                                                            |
| **Ready to publish** but no automatic comment        | The run may be manual, a switch may have been off when it completed, or publication may have been held/failed. Read the chat error and publish manually if appropriate. |
| Follow-up rejected after cleanup                     | Read the saved chat in Activity. Reconnect the repository and start another review if the PR is still open.                                                             |
| **Orbit is already running for this workspace**      | Close the existing instance. Removing the lock file is not the normal recovery path.                                                                                    |
| Cannot read saved state                              | The app leaves the invalid file untouched and cannot start normally. Preserve the app-data directory for recovery; it does not offer an automatic repair/reset flow.    |
| Could not save local state; automation paused        | Resolve the app-data write failure before re-enabling automation. Unsaved changes are not guaranteed to survive exit.                                                   |

For a connection check from a source checkout, `npm run diagnose` uses temporary state and does not load your configured repositories. It checks authentication, model discovery, and repository listing; it does not prove that a repository's setup command or a live review will succeed.

## Verification record

The behaviors above were traced from UI controls through the native command handlers to the Rust implementation. Existing prose documentation and the original design specification were not used as behavioral authority. This work did not start a live review, run a repository's setup command, grant an approval, or publish a GitHub comment.

The links below point to the implementation in this checkout, including the usability/storage changes.

| Area                                                                    | Code inspected                                                                                                                                                    |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Navigation and PR details                                               | [App.tsx](../src/App.tsx), [Inbox.tsx](../src/Inbox.tsx), [Detail.tsx](../src/Detail.tsx), [Activity.tsx](../src/Activity.tsx)                                    |
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
