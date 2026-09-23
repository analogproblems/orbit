import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { useOrbit } from "./store";
import { canPublish } from "./types";

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
  it("blocks publishing a stale base or head", () => {
    const s = useOrbit.getState().snapshot;
    const r = s.reviews[0];
    const p = s.prs[0];
    expect(canPublish(r, p)).toBe(true);
    expect(canPublish(r, { ...p, headSha: "f".repeat(40) })).toBe(false);
    expect(canPublish(r, { ...p, baseSha: "f".repeat(40) })).toBe(false);
  });
});
