import { create } from "zustand";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { freshDemo, demoReport } from "./demo";
import {
  defaults,
  canPublish,
  activeStatuses,
  type Snapshot,
  type Settings,
  type ChangedFile,
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
interface Store {
  snapshot: Snapshot;
  loaded: boolean;
  error: string | null;
  toast: string | null;
  init: () => Promise<() => void>;
  action: <T = void>(
    command: string,
    args?: Record<string, unknown>,
  ) => Promise<T | undefined>;
  clearError: () => void;
}
let demo: Snapshot;
const timers = new Map<string, ReturnType<typeof setTimeout>>();
function commit() {
  localStorage.setItem("orbit-demo-v1", JSON.stringify(demo));
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
                (repo) => repo.name === pr?.repo && repo.automatic,
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
  demo.reviews.unshift({
    id: crypto.randomUUID(),
    prId,
    headSha: pr.headSha,
    baseSha: pr.baseSha,
    status: "queued",
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
  if (command === "save_settings") {
    demo.settings = args.settings as Settings;
    commit();
    if (demo.settings.automatic)
      for (const p of demo.prs.filter(
        (p) =>
          !p.draft &&
          demo.settings.repositories.some(
            (r) => r.name === p.repo && r.automatic,
          ) &&
          !demo.reviews.some((r) => r.prId === p.id && r.headSha === p.headSha),
      ))
        demoRun(p.id, true);
    demoSchedule();
    return;
  }
  if (command === "start_review") {
    demoRun(args.prId as string);
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
    commit();
    return;
  }
  if (command === "send_message") {
    const r = demo.reviews.find((r) => r.id === args.reviewId)!;
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
  if (command === "connect_services") {
    commit();
    return;
  }
  throw Error("This action is available in the desktop app.");
}
export const useOrbit = create<Store>((set) => ({
  snapshot: empty,
  loaded: false,
  error: null,
  toast: null,
  clearError: () => set({ error: null }),
  init: async () => {
    if (!desktop) {
      try {
        demo =
          JSON.parse(localStorage.getItem("orbit-demo-v1") || "null") ||
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
  action: async <T>(command: string, args: Record<string, unknown> = {}) => {
    try {
      set({ error: null });
      const result = desktop
        ? await invoke<T>(command, args)
        : ((await demoAction(command, args)) as T);
      return result;
    } catch (e) {
      set({ error: String(e) });
      return undefined;
    }
  },
}));
