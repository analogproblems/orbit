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
    await store.action("save_settings", {
      settings: {
        ...store.snapshot.settings,
        automatic: true,
        autoPost: true,
        repositories: [],
      },
    });
    await store.action("start_review", { prId: store.snapshot.prs[1].id });
    await vi.advanceTimersByTimeAsync(6000);
    expect(useOrbit.getState().snapshot.reviews[0].status).toBe("ready");
  });
  it("honors queue capacity and pauses automatic jobs when autopilot is disabled", async () => {
    const store = useOrbit.getState();
    await store.action("save_settings", {
      settings: { ...store.snapshot.settings, automatic: true, concurrency: 1 },
    });
    let s = useOrbit.getState().snapshot;
    expect(s.reviews.filter((r) => r.status === "preparing")).toHaveLength(1);
    expect(s.reviews.filter((r) => r.status === "queued")).toHaveLength(1);
    await store.action("save_settings", {
      settings: { ...s.settings, automatic: false },
    });
    await vi.advanceTimersByTimeAsync(10000);
    s = useOrbit.getState().snapshot;
    expect(s.reviews.filter((r) => r.status === "queued")).toHaveLength(1);
    expect(s.reviews.filter((r) => r.status === "posted")).toHaveLength(0);
  });
  it("automatically reviews only pull requests from repository insiders", async () => {
    const store = useOrbit.getState();
    await store.action("save_settings", {
      settings: {
        ...store.snapshot.settings,
        automatic: true,
        autoPost: true,
        concurrency: 4,
        repositories: store.snapshot.settings.repositories.map((r) => ({
          ...r,
          automatic: true,
        })),
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
