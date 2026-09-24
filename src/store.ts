import { create } from "zustand";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { freshDemo, demoReport, demoRepositories } from "./demo";
import {
  defaults,
  canPublish,
  activeStatuses,
  sameRepo,
  trustedSource,
  type Snapshot,
  type Settings,
  type ChangedFile,
  type ActionResult,
  type SettingsPatch,
  type Repository,
  type StorageUsage,
  followUpUnavailable,
  canExport,
} from "./types";
export const desktop = isTauri();
const empty: Snapshot = {
  prs: [],
  reviews: [],
  settings: defaults,
  models: [],
  approvals: [],
  polling: false,
  codexReady: false,
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
let mutationTail: Promise<unknown> = Promise.resolve();
const timers = new Map<string, ReturnType<typeof setTimeout>>();
function commit() {
  localStorage.setItem("orbit-demo-v3", JSON.stringify(demo));
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
    model: demo.settings.model || "codex-default",
    effort: demo.settings.effort || "medium",
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
async function demoAction(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  if (command === "reset_demo") {
    timers.forEach(clearTimeout);
    timers.clear();
    demo = freshDemo();
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
    if (command === "update_settings") {
      const patch = args.patch as SettingsPatch;
      if (
        Object.keys(patch).some(
          (key) => key === "repositories" || !(key in next),
        )
      )
        throw Error("Unknown settings field.");
      Object.assign(next, patch);
    }
    if (command === "save_repository") {
      const repo = args.repository as Repository;
      if (!repo.path.startsWith("/"))
        throw Error("Use an absolute checkout path.");
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
    commit();
    if (demo.settings.automatic)
      for (const p of demo.prs.filter(
        (p) =>
          !p.draft &&
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
  if (command === "cancel_review") {
    const r = demo.reviews.find((r) => r.id === args.reviewId)!;
    clearTimeout(timers.get(r.id));
    timers.delete(r.id);
    r.status = "cancelled";
    demoSchedule();
    return;
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
    const content = `# Orbit review: ${pr.title}\n\nPR: ${pr.id} (${pr.url})\n\nCreated: ${r.createdAt}\n\nHead: ${r.headSha}\n\nBase: ${r.baseSha}\n\nModel: ${r.model || "Codex default"} · Effort: ${r.effort || "Model default"}\n\n${r.commentUrl ? `Published comment: ${r.commentUrl}\n\n` : ""}${r.report}\n`;
    const name = `orbit-${pr.repo.replaceAll("/", "-")}-pr-${pr.number}-${r.headSha.slice(0, 7)}.md`;
    const url = URL.createObjectURL(
      new Blob([content], { type: "text/markdown;charset=utf-8" }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = name;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    return name;
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
  if (command === "list_repositories") return demoRepositories;
  if (command === "connect_services") {
    commit();
    return;
  }
  throw Error("This action is available in the desktop app.");
}
export const useOrbit = create<Store>((set, get) => ({
  snapshot: empty,
  loaded: false,
  error: null,
  saves: {},
  trustedInput: "",
  setTrustedInput: (trustedInput) => set({ trustedInput }),
  chatDrafts: {},
  setChatDraft: (id, text) =>
    set((s) => ({ chatDrafts: { ...s.chatDrafts, [id]: text } })),
  mutate: (command, args, key) => {
    const pending: SaveState = { state: "saving", command, args };
    set((s) => ({ saves: { ...s.saves, [key]: pending } }));
    const task = mutationTail.then(async () => {
      const result = await get().action<Settings>(command, args, {
        quiet: true,
      });
      set((s) =>
        s.saves[key] === pending
          ? {
              saves: {
                ...s.saves,
                [key]: {
                  state: result.ok ? "saved" : "error",
                  error: result.ok ? undefined : result.error,
                  command,
                  args,
                },
              },
            }
          : {},
      );
      return result;
    });
    mutationTail = task.catch(() => {});
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
    if (!desktop) {
      try {
        demo =
          JSON.parse(localStorage.getItem("orbit-demo-v3") || "null") ||
          freshDemo();
        demo.reviews.forEach((r) => {
          if (activeStatuses.includes(r.status) && !timers.has(r.id)) {
            r.status = "cancelled";
            r.error = "Demo was restarted.";
          }
        });
      } catch {
        demo = freshDemo();
      }
      set({ snapshot: demo, loaded: true });
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
        : ((await demoAction(command, args)) as T);
      return { ok: true, data: result };
    } catch (e) {
      if (!options.quiet) set({ error: String(e) });
      return { ok: false, error: String(e) };
    }
  },
}));
