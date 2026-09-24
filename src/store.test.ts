import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { useOrbit } from "./store";
import { canPublish, publishTarget, trustedSource } from "./types";

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
beforeEach(async () => {
  vi.useFakeTimers();
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => storage.get(k) ?? null,
    setItem: (k: string, v: string) => storage.set(k, v),
  });
  await useOrbit.getState().init();
  await useOrbit.getState().action("reset_demo");
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
describe("review interactions", () => {
  it("stopping a review prevents late completion", async () => {
    const store = useOrbit.getState();
    await store.action("start_review", { prId: store.snapshot.prs[1].id });
    const id = useOrbit.getState().snapshot.reviews[0].id;
    await store.action("cancel_review", { reviewId: id });
    await vi.advanceTimersByTimeAsync(10000);
    const r = useOrbit.getState().snapshot.reviews.find((r) => r.id === id)!;
    expect(r.status).toBe("cancelled");
    expect(r.report).toBe("");
  });
  it("keeps manual reviews unpublished even when automatic posting is enabled", async () => {
    const store = useOrbit.getState();
    for (const repo of store.snapshot.settings.repositories) {
      await store.action("disconnect_repository", { name: repo.name });
    }
    await store.action("update_settings", {
      patch: {
        automatic: true,
        autoPost: true,
      },
    });
    await store.action("start_review", { prId: store.snapshot.prs[1].id });
    await vi.advanceTimersByTimeAsync(6000);
    expect(useOrbit.getState().snapshot.reviews[0].status).toBe("ready");
  });
  it("honors queue capacity and pauses automatic jobs when autopilot is disabled", async () => {
    const store = useOrbit.getState();
    await store.action("update_settings", {
      patch: { automatic: true, concurrency: 1 },
    });
    let s = useOrbit.getState().snapshot;
    expect(s.reviews.filter((r) => r.status === "preparing")).toHaveLength(1);
    expect(s.reviews.filter((r) => r.status === "queued")).toHaveLength(1);
    await store.action("update_settings", {
      patch: { automatic: false },
    });
    await vi.advanceTimersByTimeAsync(10000);
    s = useOrbit.getState().snapshot;
    expect(s.reviews.filter((r) => r.status === "queued")).toHaveLength(1);
    expect(s.reviews.filter((r) => r.status === "posted")).toHaveLength(0);
  });
  it("automatically reviews only pull requests from repository insiders", async () => {
    const store = useOrbit.getState();
    for (const repo of store.snapshot.settings.repositories) {
      await store.action("set_repository_automatic", {
        name: repo.name,
        automatic: true,
      });
    }
    await store.action("update_settings", {
      patch: {
        automatic: true,
        autoPost: true,
        concurrency: 4,
      },
    });
    await vi.advanceTimersByTimeAsync(6000);
    const reviews = useOrbit.getState().snapshot.reviews;
    const review = (id: string) => reviews.find((r) => r.prId === id);
    expect(review("acme/api#82")?.status).toBe("posted");
    expect(review("acme/api#79")).toBeUndefined();
  });
  it("treats forks and outside contributors as untrusted", () => {
    const [bot, p] = useOrbit.getState().snapshot.prs;
    expect(trustedSource(p)).toBe(true);
    expect(trustedSource(bot)).toBe(false);
    expect(trustedSource(bot, ["Claude[bot]"])).toBe(true);
    expect(
      trustedSource({ ...bot, headRepo: "someone/orbit" }, ["claude[bot]"]),
    ).toBe(false);
    // After a rename, GitHub reports the new name for both sides.
    expect(
      trustedSource({ ...p, headRepo: "acme/new", baseRepo: "acme/new" }),
    ).toBe(true);
    expect(trustedSource({ ...p, headRepo: "someone/orbit" })).toBe(false);
    expect(trustedSource({ ...p, authorAssociation: "CONTRIBUTOR" })).toBe(
      false,
    );
    expect(trustedSource({ ...p, headRepo: undefined })).toBe(false);
  });
  it("publishes PRs that left the inbox only if their repository was disconnected", () => {
    const s = useOrbit.getState().snapshot;
    const pr = s.prs[0];
    const closed = { ...s, prs: s.prs.slice(1), historyPrs: [pr] };
    expect(publishTarget(s, pr)).toBe(pr);
    expect(publishTarget(closed, pr)).toBeUndefined();
    const disconnected = {
      ...closed,
      settings: {
        ...s.settings,
        repositories: s.settings.repositories.filter((r) => r.name !== pr.repo),
      },
    };
    expect(publishTarget(disconnected, pr)).toBe(pr);
  });
  it("blocks publishing a stale base or head", () => {
    const s = useOrbit.getState().snapshot;
    const r = s.reviews[0];
    const p = s.prs[0];
    expect(canPublish(r, p)).toBe(true);
    expect(canPublish(r, { ...p, headSha: "f".repeat(40) })).toBe(false);
    expect(canPublish(r, { ...p, baseSha: "f".repeat(40) })).toBe(false);
  });
});

describe("older demo storage", () => {
  it("backfills simulated chat metadata without restoring deliberately removed worktrees", async () => {
    const old = structuredClone(useOrbit.getState().snapshot);
    const seeded = old.reviews[0];
    delete seeded.threadId;
    const legacyRun = { ...seeded, id: "legacy-run", worktree: undefined };
    const removed = {
      ...seeded,
      id: "removed-run",
      threadId: "known-thread",
      worktree: undefined,
    };
    old.reviews.push(legacyRun, removed);
    localStorage.setItem("orbit-demo-v3", JSON.stringify(old));
    await useOrbit.getState().init();
    const s = useOrbit.getState().snapshot;
    expect(s.reviews[0].threadId).toBeTruthy();
    expect(s.reviews[1].worktree).toBeTruthy();
    expect(s.reviews[2].worktree).toBeUndefined();
    expect(s.reviews[0].report).toBe(seeded.report);
    expect(s.reviews[0].messages).toEqual(seeded.messages);
    const sent = await useOrbit
      .getState()
      .action("send_message", { reviewId: legacyRun.id, text: "Follow-up" });
    expect(sent.ok).toBe(true);
    await useOrbit.getState().init();
    expect(useOrbit.getState().snapshot.reviews[2].worktree).toBeUndefined();
  });
});

describe("save feedback", () => {
  it("expires successful settings and repository feedback without dismissing later failures", async () => {
    useOrbit.setState({ saves: {} });
    const store = useOrbit.getState();
    const repo = store.snapshot.settings.repositories[0];
    await store.updateSettings({ concurrency: 3 });
    await store.mutate(
      "set_repository_automatic",
      { name: repo.name, automatic: false },
      `repository:${repo.name}`,
    );
    expect(useOrbit.getState().saves.concurrency.state).toBe("saved");
    await vi.advanceTimersByTimeAsync(2500);
    expect(useOrbit.getState().saves).toEqual({});

    await store.updateSettings({ trustedAuthors: ["bot"] });
    await vi.advanceTimersByTimeAsync(1000);
    const failed = await store.updateSettings({
      trustedAuthors: ["invalid/name"],
    });
    expect(failed.ok).toBe(false);
    await vi.advanceTimersByTimeAsync(5000);
    expect(useOrbit.getState().saves.trustedAuthors.state).toBe("error");
    expect(useOrbit.getState().saves.trustedAuthors.args.patch).toEqual({
      trustedAuthors: ["invalid/name"],
    });
  });
});
