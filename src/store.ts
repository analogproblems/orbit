import { create } from "zustand";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  demoAnswer,
  demoFixed,
  demoTypedAnswer,
  demoWorkspace,
  freshDemo,
  demoFindings,
  demoReport,
  demoRepositories,
  demoScan,
  demoScanFindings,
  demoScanReport,
  resolveDemoModelSelection,
} from "./demo";
import { call, download, follow, lostConnection, readToken } from "./remote";
import {
  buddyOf,
  defaultWriteUps,
  maxQuestion,
  memoryLimits,
  questionLabels,
  type BuddyNote,
  type Question,
  defaults,
  selectionError,
  withPatch,
  withRepositoryPatch,
  providerLabels,
  providerModels,
  providerSelection,
  repoProvider,
  reviewerSelectionUnavailable,
  chatSelection,
  chatModelUnavailable,
  reportSelectionLabel,
  canPublish,
  activeStatuses,
  sameRepo,
  trustedSource,
  scanActive,
  type ScanMode,
  type ScanPluginStatus,
  type SecurityScan,
  type Snapshot,
  type Settings,
  type ChangedFile,
  type ActionResult,
  type PublishPlan,
  type SettingsPatch,
  type Repository,
  type StorageUsage,
  type CleanupResult,
  followUpUnavailable,
  canExport,
} from "./types";
/** Running in the native desktop window. */
export const desktop = isTauri();
const remoteToken = desktop ? null : readToken();
/** Running in a browser connected to a desktop Orbit through an access link. */
export const remote = remoteToken !== null;
/** Connected to a real workspace, rather than the browser demo. */
export const live = desktop || remote;
/** Opens a GitHub link: with the system browser from the desktop window, in a
 * new tab in a browser. */
export function openLink(url: string) {
  if (desktop) void useOrbit.getState().action("open_github", { url });
  else window.open(url, "_blank", "noopener,noreferrer");
}
const empty: Snapshot = {
  prs: [],
  reviews: [],
  settings: defaults,
  models: [],
  approvals: [],
  polling: false,
  codexReady: false,
  modelCatalogReady: false,
};
export interface SaveState {
  state: "saving" | "saved" | "error";
  error?: string;
  command: string;
  args: Record<string, unknown>;
}
interface Store {
  snapshot: Snapshot;
  loaded: boolean;
  error: string | null;
  saves: Record<string, SaveState>;
  cleanup: { running: boolean; results: CleanupResult[]; error?: string };
  removeWorktrees: (ids: string[]) => Promise<ActionResult<CleanupResult[]>>;
  trustedInput: string;
  setTrustedInput: (text: string) => void;
  chatDrafts: Record<string, string>;
  setChatDraft: (id: string, text: string) => void;
  mutate: (
    command: string,
    args: Record<string, unknown>,
    key: string,
  ) => Promise<ActionResult<Settings>>;
  updateSettings: (
    patch: SettingsPatch,
    key?: string,
  ) => Promise<ActionResult<Settings>>;
  retrySave: (key: string) => Promise<ActionResult<Settings>>;
  init: () => Promise<() => void>;
  action: <T = void>(
    command: string,
    args?: Record<string, unknown>,
    options?: { quiet?: boolean },
  ) => Promise<ActionResult<T>>;
  clearError: () => void;
}
let demo: Snapshot;
// Repository validation can wait on a slow filesystem. Keep its ordered lane
// independent from the global settings lane, so disabling Autopilot stays fast.
const mutationTails = new Map<string, Promise<unknown>>();
const timers = new Map<string, ReturnType<typeof setTimeout>>();
function commit() {
  try {
    localStorage.setItem("orbit-demo-v3", JSON.stringify(demo));
  } catch (error) {
    demo = structuredClone(useOrbit.getState().snapshot);
    throw error;
  }
  useOrbit.setState({ snapshot: structuredClone(demo) });
}
function demoSchedule() {
  const running = demo.reviews.filter(
    (r) => activeStatuses.includes(r.status) && r.status !== "queued",
  ).length;
  const jobs = demo.reviews
    .filter(
      (r) =>
        r.status === "queued" &&
        (!r.automatic ||
          (demo.settings.automatic &&
            demo.settings.repositories.some(
              (repo) =>
                repo.automatic &&
                demo.prs.some((p) => p.id === r.prId && p.repo === repo.name),
            ))),
    )
    .reverse()
    .slice(0, Math.max(0, demo.settings.concurrency - running));
  for (const r of jobs) {
    r.status = "preparing";
    timers.set(
      r.id,
      setTimeout(() => {
        const error = selectionError(demo.models, r);
        if (error) {
          r.status = "failed";
          r.error = error;
          timers.delete(r.id);
          commit();
          demoSchedule();
          return;
        }
        r.confirmedReviewSelection = { model: r.model, effort: r.effort };
        r.status = "reviewing";
        r.messages.push({
          id: crypto.randomUUID(),
          role: "agent",
          text: "Demo review in progress. Inspecting the diff, tracing affected code paths, and checking the tests.",
          createdAt: new Date().toISOString(),
        });
        commit();
        timers.set(
          r.id,
          setTimeout(() => {
            r.status = "ready";
            r.report = demoReport;
            r.findings = demoFindings(r.id);
            r.verdict = "changes_requested";
            const earlier = demo.reviews.find(
              (p) => p.id !== r.id && p.prId === r.prId && p.findings?.length,
            );
            if (earlier?.findings) {
              r.previousRound = {
                reviewId: earlier.id,
                headSha: earlier.headSha,
                verdict: earlier.verdict ?? null,
                findings: earlier.findings,
                decisions: earlier.priorFindings ?? [],
                commentsIncluded: 1,
                commentsOmitted: 0,
              };
              r.priorFindings = [
                {
                  id: earlier.findings[0].id,
                  status: "fixed",
                  note: "Defaults now merge with the existing repository entries.",
                },
                {
                  id: earlier.findings[1].id,
                  status: "declined_accepted",
                  note: "The author explained that the queue already checks the workspace.",
                },
              ];
              // The earlier findings are settled, so this round finds nothing new.
              r.findings = [];
              r.verdict = "clean";
            }
            const pr = demo.prs.find((p) => p.id === r.prId);
            if (
              r.automatic &&
              demo.settings.automatic &&
              demo.settings.autoPost &&
              demo.settings.repositories.some(
                (repo) =>
                  !!pr && sameRepo(repo.name, pr.repo) && repo.automatic,
              ) &&
              canPublish(r, pr)
            )
              r.status = "posted";
            r.updatedAt = new Date().toISOString();
            r.messages.push({
              id: crypto.randomUUID(),
              role: "agent",
              text: demoReport,
              createdAt: r.updatedAt,
            });
            timers.delete(r.id);
            commit();
            demoSchedule();
          }, 4200),
        );
      }, 1400),
    );
  }
  commit();
}
function demoRun(prId: string, automatic = false) {
  const pr = demo.prs.find((p) => p.id === prId);
  if (!pr) throw Error("Unknown pull request.");
  const provider = repoProvider(demo, pr.repo);
  const unavailable = reviewerSelectionUnavailable(demo, provider);
  if (unavailable) throw Error(unavailable);
  const selection = providerSelection(demo, provider);
  if (
    demo.reviews.some(
      (r) => r.prId === prId && activeStatuses.includes(r.status),
    )
  )
    throw Error("This pull request already has an active review.");
  const now = new Date().toISOString();
  const id = crypto.randomUUID();
  demo.reviews.unshift({
    id,
    prId,
    headSha: pr.headSha,
    baseSha: pr.baseSha,
    status: "queued",
    threadId: crypto.randomUUID(),
    worktree: "demo-worktree",
    provider,
    model: selection.model,
    effort: selection.effort,
    createdAt: now,
    updatedAt: now,
    messages: [
      {
        id: crypto.randomUUID(),
        role: "system",
        text: "Demo: preparing an isolated repository environment…",
        createdAt: now,
      },
    ],
    report: "",
    automatic,
    isReview: true,
  });
  demoSchedule();
  return id;
}
/** Adds a note to Bit's feed in the demo, keeping the newest 50. */
function demoNote(note: Partial<BuddyNote> & Pick<BuddyNote, "kind" | "text">) {
  const feed = (demo.buddyFeed ??= { notes: [] });
  const id = Math.max(0, ...feed.notes.map((n) => n.id)) + 1;
  feed.notes.push({
    prId: null,
    reviewId: null,
    repo: null,
    ...note,
    id,
    at: new Date().toISOString(),
  });
  feed.notes = feed.notes.slice(-50);
}
/** Where the demo pretends Orbit keeps its own copy of `name`. */
const demoCopy = (name: string) =>
  `/Users/demo/Library/Application Support/Orbit/clones/${name}`;
async function demoAction(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  if (command === "ask_buddy") {
    const question = args.question as Question;
    demoNote({
      kind: "answer",
      question: questionLabels[question],
      ...demoAnswer(demo, question),
    });
    commit();
    return;
  }
  if (command === "ask_buddy_anything") {
    const question = String(args.question ?? "").trim();
    if (!question || [...question].length > maxQuestion)
      throw Error(`Ask a question of 1 to ${maxQuestion} characters.`);
    const buddy = buddyOf(demo.settings);
    if (!buddy.enabled) throw Error(`Turn ${buddy.name} on in Settings first.`);
    if (demo.buddyFeed?.notes.some((n) => n.pending))
      throw Error(`${buddy.name} is still answering your last question.`);
    demoNote({ kind: "answer", question, text: "", pending: true });
    const id = demo.buddyFeed!.notes.at(-1)!.id;
    commit();
    timers.set(
      `ask-${id}`,
      setTimeout(() => {
        timers.delete(`ask-${id}`);
        const note = demo.buddyFeed?.notes.find(
          (n) => n.id === id && n.pending,
        );
        if (!note) return;
        const answer = demoTypedAnswer();
        const memory = (demo.buddyFeed!.memory ??= {
          general: [],
          repositories: {},
        });
        // As Orbit does, a line already kept isn't remembered again.
        answer.memory = answer.memory?.filter(
          (c) =>
            !memory.general.includes(c.line) &&
            memory.general.length < memoryLimits.general,
        );
        for (const change of answer.memory ?? [])
          memory.general.push(change.line);
        Object.assign(note, {
          ...answer,
          pending: false,
          at: new Date().toISOString(),
        });
        commit();
      }, 1500),
    );
    return;
  }
  if (command === "use_proposal") {
    const find = () =>
      demo.buddyFeed?.notes
        .find((n) => n.id === args.noteId)
        ?.proposals?.find((p) => p.id === args.proposalId);
    const p = find();
    if (!p) throw Error("This button isn't there any more.");
    if (p.done) throw Error("That's done already.");
    if (p.action === "continue_loop")
      await demoAction("continue_loop", { prId: p.prId, rounds: 2 });
    else if (p.action === "stop_loop")
      await demoAction("stop_loop", { prId: p.prId });
    else if (p.action === "review_again")
      await demoAction("start_review", { prId: p.prId });
    else if (p.action === "publish")
      await demoAction("publish_review", {
        reviewId: p.reviewId,
        allowSensitive: false,
      });
    else if (p.action === "change_settings")
      await demoAction("update_settings", { patch: p.settings });
    else if (p.action === "set_up_repository") {
      const name = p.repo ?? "";
      const connected = demo.settings.repositories.find((r) =>
        sameRepo(r.name, name),
      );
      // As Orbit does, a repository it connects gets its own copy.
      const repo = connected ?? {
        name,
        path: demoCopy(name),
        automatic: false,
        setupCommand: "",
        networkAccess: false,
        roundLimit: 4,
        approvalMinutes: 0,
        keepUpdated: true,
        provider: "codex" as const,
        devContainer: { enabled: false, checkCommand: "" },
      };
      await demoAction("save_repository", {
        repository: withRepositoryPatch(repo, p.repository ?? {}),
        original: connected?.name ?? null,
      });
    } else demo.lastPoll = new Date().toISOString();
    // The demo may have been replaced while the control ran.
    const done = find();
    if (done) done.done = true;
    commit();
    return;
  }
  if (command === "forget_memory") {
    const memory = demo.buddyFeed?.memory;
    const repo = args.repo as string | null;
    const lines = repo ? memory?.repositories[repo] : memory?.general;
    const at = lines?.indexOf(String(args.line)) ?? -1;
    if (!memory || !lines || at < 0)
      throw Error("That line isn't there any more.");
    lines.splice(at, 1);
    if (repo && !lines.length) delete memory.repositories[repo];
    commit();
    return;
  }
  if (command === "clear_memory") {
    if (demo.buddyFeed)
      demo.buddyFeed.memory = { general: [], repositories: {} };
    commit();
    return;
  }
  if (command === "use_fix") {
    const item = demo.attention?.find((a) => a.key === args.key);
    if (!item) throw Error("This doesn't need you any more.");
    if (!item.fix)
      throw Error(
        "There's no button for this one. Open it to see what it needs.",
      );
    if (item.fix === "continue_loop")
      await demoAction("continue_loop", { prId: item.prId, rounds: 2 });
    else if (item.fix === "review_again")
      await demoAction("start_review", { prId: item.prId });
    else if (item.fix === "publish")
      await demoAction("publish_review", {
        reviewId: item.reviewId,
        allowSensitive: false,
      });
    else if (item.fix === "turn_on_autopilot")
      await demoAction("update_settings", { patch: { automatic: true } });
    else demo.lastPoll = new Date().toISOString();
    demo.attention = demo.attention?.filter((a) => a.key !== item.key);
    demoNote({
      kind: "fixed",
      text: demoFixed(demo, item.fix, item.prId),
      prId: item.prId,
      reviewId: item.reviewId,
      repo: item.repo,
    });
    commit();
    return;
  }
  if (command === "reset_demo") {
    timers.forEach(clearTimeout);
    timers.clear();
    demo = demoWorkspace();
    commit();
    return;
  }
  if (command === "poll_now") {
    demo.lastPoll = new Date().toISOString();
    commit();
    return;
  }
  if (
    [
      "update_settings",
      "save_repository",
      "set_repository_automatic",
      "disconnect_repository",
    ].includes(command)
  ) {
    const next = structuredClone(demo.settings);
    // Demos saved before Bit or write-ups have no settings for them yet.
    next.buddy = buddyOf(next);
    next.writeUps ??= { ...defaultWriteUps };
    if (command === "update_settings") {
      const patch = args.patch as SettingsPatch;
      if (
        Object.keys(patch).some(
          (key) => key === "repositories" || !(key in next),
        )
      )
        throw Error("Unknown settings field.");
      Object.assign(next, withPatch(next, patch));
      if (patch.buddy?.name !== undefined) {
        next.buddy.name = patch.buddy.name.trim();
        if (!next.buddy.name) throw Error("Give your buddy a name.");
        if ([...next.buddy.name].length > 24)
          throw Error("Keep your buddy's name to 24 characters.");
      }
      if (patch.model !== undefined || patch.effort !== undefined) {
        const error = selectionError(demo.models, next);
        if (error) throw Error(error);
      }
      for (const provider of ["claude", "antigravity"] as const) {
        const [model, effort] =
          provider === "claude"
            ? [patch.claudeModel, patch.claudeEffort]
            : [patch.antigravityModel, patch.antigravityEffort];
        if (model === undefined && effort === undefined) continue;
        const error = selectionError(
          providerModels(demo, provider),
          providerSelection({ ...demo, settings: next }, provider),
          provider,
        );
        if (error) throw Error(error);
      }
    }
    if (command === "save_repository") {
      const repo = { ...(args.repository as Repository) };
      // As in Orbit, a new repository without a folder gets Orbit's own
      // copy, kept up to date.
      if (!args.original && !repo.path.trim()) {
        if (next.repositories.some((r) => sameRepo(r.name, repo.name)))
          throw Error("Each repository can only be configured once.");
        repo.path = demoCopy(repo.name);
        repo.keepUpdated = true;
      }
      // As in Orbit, a checkout is checked only when it changes.
      const before = next.repositories.find(
        (r) => args.original && sameRepo(r.name, args.original as string),
      );
      if (before?.path !== repo.path && !repo.path.startsWith("/"))
        throw Error("Use an absolute checkout path.");
      if (
        repo.provider &&
        repo.provider !== "codex" &&
        !repo.devContainer?.enabled &&
        repo.setupCommand.trim()
      )
        throw Error(
          `${providerLabels[repo.provider]} reviews can't run a setup command outside a dev container. Clear the setup command for ${repo.name}, run it in the repository's dev container, or review it with Codex.`,
        );
      if (
        repo.devContainer?.enabled &&
        !repo.devContainer.reviewerTool &&
        !repo.setupCommand.trim() &&
        !repo.devContainer.checkCommand.trim()
      )
        throw Error(
          `Add a setup or check command for ${repo.name}'s dev container, let the reviewer run commands there, or turn it off.`,
        );
      if (args.original)
        next.repositories = next.repositories.map((r) =>
          sameRepo(r.name, args.original as string) ? repo : r,
        );
      else next.repositories.push(repo);
    }
    if (command === "disconnect_repository")
      next.repositories = next.repositories.filter(
        (r) => !sameRepo(r.name, args.name as string),
      );
    if (command === "set_repository_automatic")
      next.repositories = next.repositories.map((r) =>
        sameRepo(r.name, args.name as string)
          ? { ...r, automatic: args.automatic as boolean }
          : r,
      );
    if (
      next.trustedAuthors.length > 50 ||
      next.trustedAuthors.some(
        (a) => !/^[a-z\d][a-z\d-]{0,38}(\[bot\])?$/i.test(a),
      )
    )
      throw Error("Enter a valid GitHub account name (at most 50 accounts).");
    demo.settings = next;
    demo.modelConfigurationError = selectionError(demo.models, next);
    if (
      command === "update_settings" &&
      ("model" in (args.patch as SettingsPatch) ||
        "effort" in (args.patch as SettingsPatch))
    )
      demo.modelSelectionNotice = undefined;
    commit();
    if (demo.settings.automatic)
      for (const p of demo.prs.filter(
        (p) =>
          !p.draft &&
          !reviewerSelectionUnavailable(demo, repoProvider(demo, p.repo)) &&
          trustedSource(p, demo.settings.trustedAuthors) &&
          demo.settings.repositories.some(
            (r) => sameRepo(r.name, p.repo) && r.automatic,
          ) &&
          !demo.reviews.some((r) => r.prId === p.id && r.headSha === p.headSha),
      ))
        demoRun(p.id, true);
    demoSchedule();
    return structuredClone(demo.settings);
  }
  if (command === "start_review") {
    return demoRun(args.prId as string);
  }
  if (command === "stop_loop" || command === "continue_loop") {
    const state = demo.loopStates?.find((l) => l.prId === args.prId);
    if (state) {
      const stop = command === "stop_loop";
      state.status = stop ? "stopped" : "waiting_for_author";
      state.reason = null;
      if (!stop) state.roundLimit = state.rounds + Number(args.rounds);
    }
    demo.attention = demo.attention?.filter((a) => a.prId !== args.prId);
    commit();
    return;
  }
  if (command === "cancel_review") {
    const r = demo.reviews.find((r) => r.id === args.reviewId)!;
    clearTimeout(timers.get(r.id));
    timers.delete(r.id);
    r.status = "cancelled";
    demoSchedule();
    return;
  }
  if (command === "publish_plan") {
    const r = demo.reviews.find((r) => r.id === args.reviewId)!;
    if (!r.findings) return null;
    return {
      placements: r.findings.map((f) => ({
        findingId: f.id,
        inline: !!f.path,
        reason: f.path ? null : "outside the repository",
      })),
      body: `## Orbit · ${providerLabels[r.provider ?? "codex"]} review\n\nReviewed head \`${r.headSha}\` against base \`${r.baseSha}\`.\n\n${r.summary ?? ""}`,
    } satisfies PublishPlan;
  }
  if (command === "publish_review") {
    const r = demo.reviews.find((r) => r.id === args.reviewId)!;
    if (
      !canPublish(
        r,
        demo.prs.find((p) => p.id === r.prId),
      )
    )
      throw Error("Only current, completed reviews can be published.");
    r.status = "posted";
    r.commentUrl = demo.prs.find((p) => p.id === r.prId)?.url;
    commit();
    return;
  }
  if (command === "get_decisions")
    return (await import("./demoDecisions")).demoDecisions();
  // The demo's history is made up; there is nothing to read again.
  if (command === "refresh_decisions") return;
  if (command === "export_decisions") {
    const { demoExport } = await import("./demoDecisions");
    const file = demoExport((args.repo as string | null) ?? null);
    download(file.fileName, file.text);
    return file.fileName;
  }
  if (command === "get_storage_usage") {
    const worktrees = demo.reviews
      .filter((r) => r.worktree)
      .map((r) => ({
        reviewId: r.id,
        bytes: 12 * 1024 * 1024,
        removable:
          !activeStatuses.includes(r.status) &&
          r.status !== "publishing" &&
          !r.cleanupPending,
      }));
    return {
      stateBytes: new TextEncoder().encode(JSON.stringify(demo)).length,
      worktreeBytes: worktrees.reduce((n, r) => n + r.bytes, 0),
      unrecognizedBytes: 0,
      unrecognized: [],
      worktrees,
      errors: [],
    } satisfies StorageUsage;
  }
  if (command === "remove_worktrees") {
    const results = (args.reviewIds as string[]).map((id) => {
      const r = demo.reviews.find((r) => r.id === id);
      if (!r || activeStatuses.includes(r.status) || r.status === "publishing")
        return { reviewId: id, error: "This run is still active." };
      r.worktree = undefined;
      return { reviewId: id, error: null };
    });
    commit();
    return results;
  }
  if (command === "export_report") {
    const r = demo.reviews.find((r) => r.id === args.reviewId);
    const pr = demo.prs.find((p) => p.id === r?.prId);
    if (!r || !pr || !canExport(r))
      throw Error("Export requires an inactive review with a report.");
    const content = `# Orbit review: ${pr.title}\n\nPR: ${pr.id} (${pr.url})\n\nCreated: ${r.createdAt}\n\nHead: ${r.headSha}\n\nBase: ${r.baseSha}\n\nReport selection: ${reportSelectionLabel(r)}\n\n${r.commentUrl ? `Published comment: ${r.commentUrl}\n\n` : ""}${r.report}\n`;
    const name = `orbit-${pr.repo.replaceAll("/", "-")}-pr-${pr.number}-${r.headSha.slice(0, 7)}.md`;
    download(name, content);
    return name;
  }
  if (command === "set_legacy_chat_model") {
    const r = demo.reviews.find((r) => r.id === args.reviewId);
    if (!r) throw Error("Unknown review");
    const selection = {
      model: args.model as string,
      effort: args.effort as string,
    };
    const error =
      chatModelUnavailable(demo, r) || selectionError(demo.models, selection);
    if (error) throw Error(error);
    if (r.model && r.effort)
      throw Error("This chat already has its original selection pinned.");
    if (
      r.legacyChatSelection &&
      (r.legacyChatSelection.model !== selection.model ||
        r.legacyChatSelection.effort !== selection.effort)
    )
      throw Error("This chat's selection is already pinned.");
    r.legacyChatSelection = selection;
    commit();
    return;
  }
  if (command === "send_message") {
    const r = demo.reviews.find((r) => r.id === args.reviewId)!;
    const reason = followUpUnavailable(demo, r);
    if (reason) throw Error(reason);
    r.messages.push(
      {
        id: crypto.randomUUID(),
        role: "user",
        text: args.text as string,
        confirmedSelection: chatSelection(r),
        delivery: "sent",
        createdAt: new Date().toISOString(),
      },
      {
        id: crypto.randomUUID(),
        role: "agent",
        text: "This is an interactive demo. In the desktop app, this message continues the Codex conversation in this review’s sandbox with the full repository context.",
        createdAt: new Date().toISOString(),
      },
    );
    commit();
    return;
  }
  if (command === "get_files")
    return [
      {
        filename: "src/config/repositories.ts",
        status: "modified",
        additions: 84,
        deletions: 12,
        patch:
          "@@ -42,6 +42,12 @@ export async function saveConfig(config) {\n   const current = await readConfig();\n-  return writeConfig(current);\n+  const updated = {\n+    ...current,\n+    defaults: config.defaults,\n+    repositories: config.repositories,\n+  };\n+  return writeConfig(updated);\n }",
      },
      {
        filename: "src/reviews/queue.ts",
        status: "modified",
        additions: 42,
        deletions: 8,
        patch:
          '@@ -68,3 +68,5 @@ async function enqueueReview(pr) {\n   const repository = findRepository(pr.repo);\n+  if (!repository) throw new Error("Configure a repository first");\n+  await queue.add({ pr, repository });\n }',
      },
      {
        filename: "tests/repositories.test.ts",
        status: "added",
        additions: 88,
        deletions: 18,
        patch:
          '@@ -0,0 +1,4 @@\n+it("persists repository configuration", async () => {\n+  await saveConfig(configuration);\n+  expect(await readConfig()).toEqual(configuration);\n+});',
      },
    ] satisfies ChangedFile[];
  if (command === "get_scan_plugin" || command === "install_scan_plugin")
    return {
      available: true,
      installed: true,
      version: "0.1.24",
    } satisfies ScanPluginStatus;
  if (command === "start_scan") {
    demo.scans ??= [];
    if (demo.scans.some(scanActive))
      throw Error(
        "A security scan is already running. Wait for it or cancel it.",
      );
    const scan: SecurityScan = {
      ...demoScan(args.repo as string, 0),
      id: `scan-${crypto.randomUUID()}`,
      mode: args.mode as ScanMode,
      status: "running",
      finishedAt: null,
      findings: [],
      hasReport: false,
      diskBytes: null,
      progress: { phase: "preflight", updatedAt: new Date().toISOString() },
    };
    demo.scans.unshift(scan);
    commit();
    // A demo scan moves through its phases in seconds rather than hours.
    const phases = ["threat_model", "discovery", "validation", "reporting"];
    const step = (index: number) => {
      const current = demo.scans?.find(
        (s) => s.id === scan.id && s.status === "running",
      );
      if (!current) return;
      if (index < phases.length) {
        current.progress = {
          phase: phases[index],
          completed: phases[index] === "discovery" ? 3 : null,
          total: phases[index] === "discovery" ? 8 : null,
          unit: "reviews",
          updatedAt: new Date().toISOString(),
        };
        timers.set(
          scan.id,
          setTimeout(() => step(index + 1), 2500),
        );
      } else {
        Object.assign(current, {
          status: "completed",
          finishedAt: new Date().toISOString(),
          findings: demoScanFindings(scan.id),
          hasReport: true,
          diskBytes: 3.2 * 1024 * 1024,
          progress: null,
        });
        timers.delete(scan.id);
      }
      commit();
    };
    timers.set(
      scan.id,
      setTimeout(() => step(0), 2500),
    );
    return scan.id;
  }
  if (command === "cancel_scan") {
    const scan = demo.scans?.find((s) => s.id === args.scanId);
    if (!scan || !scanActive(scan)) throw Error("This scan is not running.");
    clearTimeout(timers.get(scan.id));
    timers.delete(scan.id);
    // The app waits for the plugin to confirm its scan stopped.
    scan.status = "cancelling";
    scan.error = null;
    commit();
    timers.set(
      scan.id,
      setTimeout(() => {
        timers.delete(scan.id);
        if (scan.status !== "cancelling") return;
        scan.status = "cancelled";
        scan.finishedAt = new Date().toISOString();
        commit();
      }, 1500),
    );
    return;
  }
  if (command === "delete_scan") {
    if (demo.scans?.some((s) => s.id === args.scanId && scanActive(s)))
      throw Error("Cancel the scan before deleting it.");
    demo.scans = demo.scans?.filter((s) => s.id !== args.scanId);
    commit();
    return;
  }
  if (command === "get_scan_report") {
    if (!demo.scans?.some((s) => s.id === args.scanId && s.hasReport))
      throw Error("This scan has no report.");
    return demoScanReport;
  }
  if (command === "list_repositories") return demoRepositories;
  if (command === "connect_services") {
    resolveDemoSelection();
    commit();
    return;
  }
  throw Error("This action is available in the desktop app.");
}
function resolveDemoSelection() {
  demo.codexReady = true;
  demo.modelCatalogReady = true;
  demo.models = freshDemo().models;
  try {
    const { selection, notice } = resolveDemoModelSelection(
      demo.settings,
      demo.models,
    );
    demo.modelConfigurationError = null;
    demo.modelSelectionNotice = notice;
    Object.assign(demo.settings, selection);
  } catch (error) {
    demo.modelConfigurationError =
      error instanceof Error ? error.message : String(error);
  }
}
async function remoteAction<T>(
  token: string,
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  if (command !== "export_report" && command !== "export_decisions")
    return call<T>(token, command, args);
  // The browser saves the file itself, as the desktop's save dialog would.
  const report = await call<{ fileName: string; text: string }>(
    token,
    command,
    args,
  );
  download(report.fileName, report.text);
  return report.fileName as T;
}
export const useOrbit = create<Store>((set, get) => ({
  snapshot: empty,
  loaded: false,
  error: null,
  saves: {},
  cleanup: { running: false, results: [] },
  removeWorktrees: async (ids) => {
    if (get().cleanup.running)
      return { ok: false, error: "Worktree removal is already running." };
    set({ cleanup: { running: true, results: [] } });
    const result = await get().action<CleanupResult[]>(
      "remove_worktrees",
      { reviewIds: ids },
      { quiet: true },
    );
    set({
      cleanup: result.ok
        ? { running: false, results: result.data }
        : { running: false, results: [], error: result.error },
    });
    return result;
  },
  trustedInput: "",
  setTrustedInput: (trustedInput) => set({ trustedInput }),
  chatDrafts: {},
  setChatDraft: (id, text) =>
    set((s) => ({ chatDrafts: { ...s.chatDrafts, [id]: text } })),
  mutate: (command, args, key) => {
    const pending: SaveState = { state: "saving", command, args };
    set((s) => ({ saves: { ...s.saves, [key]: pending } }));
    const lane = command === "update_settings" ? "settings" : "repositories";
    const task = (mutationTails.get(lane) ?? Promise.resolve()).then(
      async () => {
        const result = await get().action<Settings>(command, args, {
          quiet: true,
        });
        const completed: SaveState = {
          state: result.ok ? "saved" : "error",
          error: result.ok ? undefined : result.error,
          command,
          args,
        };
        set((s) =>
          s.saves[key] === pending
            ? {
                saves: {
                  ...s.saves,
                  [key]: completed,
                },
              }
            : {},
        );
        if (result.ok) {
          setTimeout(() => {
            set((s) => {
              // A previous success must not dismiss a newer pending write or error.
              if (s.saves[key] !== completed) return {};
              const saves = { ...s.saves };
              delete saves[key];
              return { saves };
            });
          }, 2500);
        }
        return result;
      },
    );
    mutationTails.set(
      lane,
      task.catch(() => {}),
    );
    return task;
  },
  updateSettings: (patch, key = Object.keys(patch)[0]) =>
    get().mutate("update_settings", { patch }, key),
  retrySave: (key) => {
    const save = get().saves[key];
    return get().mutate(save.command, save.args, key);
  },
  clearError: () => set({ error: null }),
  init: async () => {
    if (remoteToken !== null) {
      return follow(
        remoteToken,
        (snapshot) => set({ snapshot, loaded: true }),
        (problem) =>
          set((s) =>
            problem
              ? { error: problem, loaded: true }
              : s.error === lostConnection
                ? { error: null }
                : {},
          ),
      );
    }
    if (!desktop) {
      try {
        demo =
          JSON.parse(localStorage.getItem("orbit-demo-v3") || "null") ||
          demoWorkspace();
        demo.reviews.forEach((r) => {
          // Older demos did not record a simulated Codex thread. Backfill those
          // runs without reviving worktrees explicitly removed by current builds
          // (which already have threadId).
          if (!r.threadId) {
            r.threadId = `demo-thread-${r.id}`;
            r.worktree ??= `demo-worktree-${r.id}`;
          }
          if (activeStatuses.includes(r.status) && !timers.has(r.id)) {
            r.status = "cancelled";
            r.error = "Demo was restarted.";
          }
        });
        for (const note of demo.buddyFeed?.notes ?? []) {
          if (!note.pending) continue;
          note.pending = false;
          note.text = "The demo restarted before I could answer. Ask me again?";
        }
      } catch {
        demo = demoWorkspace();
      }
      resolveDemoSelection();
      commit();
      set({ snapshot: structuredClone(demo), loaded: true });
      return () => {};
    }
    const unlisten = await listen<Snapshot>("orbit-state", (event) =>
      set({ snapshot: event.payload, loaded: true }),
    );
    try {
      set({ snapshot: await invoke<Snapshot>("get_snapshot"), loaded: true });
    } catch (e) {
      set({ error: String(e), loaded: true });
    }
    return unlisten;
  },
  action: async <T>(
    command: string,
    args: Record<string, unknown> = {},
    options: { quiet?: boolean } = {},
  ): Promise<ActionResult<T>> => {
    try {
      if (!options.quiet) set({ error: null });
      const result = desktop
        ? await invoke<T>(command, args)
        : remoteToken !== null
          ? await remoteAction<T>(remoteToken, command, args)
          : ((await demoAction(command, args)) as T);
      return { ok: true, data: result };
    } catch (e) {
      if (!options.quiet) set({ error: String(e) });
      return { ok: false, error: String(e) };
    }
  },
}));
