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
    // Without the demo's loops, so both orbit PRs are new to autopilot.
    const old = structuredClone(useOrbit.getState().snapshot);
    old.reviews = old.reviews.filter((r) => !r.automatic);
    localStorage.setItem("orbit-demo-v3", JSON.stringify(old));
    await useOrbit.getState().init();
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
    const run = (id: string) =>
      useOrbit.getState().snapshot.reviews.find((r) => r.id === id)!;
    expect(s.reviews[0].threadId).toBeTruthy();
    expect(run(legacyRun.id).worktree).toBeTruthy();
    expect(run(removed.id).worktree).toBeUndefined();
    expect(s.reviews[0].report).toBe(seeded.report);
    expect(s.reviews[0].messages).toEqual(seeded.messages);
    const sent = await useOrbit
      .getState()
      .action("send_message", { reviewId: legacyRun.id, text: "Follow-up" });
    expect(sent.ok).toBe(true);
    await useOrbit.getState().init();
    expect(run(removed.id).worktree).toBeUndefined();
  });
});

describe("demo buddy", () => {
  it("gives older demos Bit and saves one buddy field at a time", async () => {
    const old = structuredClone(useOrbit.getState().snapshot);
    delete old.settings.buddy;
    localStorage.setItem("orbit-demo-v3", JSON.stringify(old));
    await useOrbit.getState().init();
    const store = useOrbit.getState();
    await store.updateSettings({ buddy: { hat: "wizard" } }, "buddy.hat");
    expect(useOrbit.getState().snapshot.settings.buddy).toMatchObject({
      name: "Bit",
      colour: "lime",
      hat: "wizard",
    });
    await store.updateSettings({ buddy: { name: "  Orbie " } }, "buddy.name");
    expect(useOrbit.getState().snapshot.settings.buddy?.name).toBe("Orbie");
    for (const [name, error] of [
      ["   ", "Give your buddy a name."],
      ["x".repeat(25), "Keep your buddy's name to 24 characters."],
    ]) {
      const result = await store.updateSettings(
        { buddy: { name } },
        "buddy.name",
      );
      expect(result.ok).toBe(false);
      expect(useOrbit.getState().saves["buddy.name"].error).toContain(error);
    }
    expect(useOrbit.getState().snapshot.settings.buddy).toMatchObject({
      name: "Orbie",
      hat: "wizard",
    });
  });

  it("answers questions and uses fixes in the feed", async () => {
    const store = useOrbit.getState();
    await store.action("reset_demo");
    const last = () => useOrbit.getState().snapshot.buddyFeed!.notes.at(-1)!;
    expect((await store.action("ask_buddy", { question: "loops" })).ok).toBe(
      true,
    );
    expect(last()).toMatchObject({
      kind: "answer",
      question: "How are my loops?",
      text: "3 loops are open: 1 needs you, 1 waiting for the author, 1 clean. Here's each one:",
    });
    const key = last().lines![0].key!;
    expect(last().lines![0].text).toMatch(
      /^orbit#\d+: round 4 of 4, needs you\. Round limit reached\.$/,
    );
    expect((await store.action("use_fix", { key })).ok).toBe(true);
    const s = useOrbit.getState().snapshot;
    expect(s.attention!.some((a) => a.key === key)).toBe(false);
    expect(last()).toMatchObject({ kind: "fixed" });
    expect(last().text).toMatch(/^Gave orbit#\d+ two more rounds!$/);
    const again = await store.action("use_fix", { key });
    expect(again).toEqual({
      ok: false,
      error: "Error: This doesn't need you any more.",
    });
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

describe("explicit demo model selections", () => {
  it("pins a queued review and every follow-up independently of later workspace edits", async () => {
    const store = useOrbit.getState();
    await store.updateSettings({ effort: "low" });
    await store.action("start_review", { prId: store.snapshot.prs[1].id });
    const id = useOrbit.getState().snapshot.reviews[0].id;
    await store.updateSettings({ effort: "high" });
    await vi.advanceTimersByTimeAsync(6000);
    let r = useOrbit.getState().snapshot.reviews.find((r) => r.id === id)!;
    expect(r.effort).toBe("low");
    expect(r.confirmedReviewSelection).toEqual({
      model: "demo-review-model",
      effort: "low",
    });
    await store.action("send_message", { reviewId: id, text: "Explain" });
    r = useOrbit.getState().snapshot.reviews.find((r) => r.id === id)!;
    expect(
      r.messages.find((m) => m.role === "user")?.confirmedSelection,
    ).toEqual(r.confirmedReviewSelection);
    const bad = await store.updateSettings({
      model: "missing-model",
      effort: "high",
    });
    expect(bad.ok).toBe(false);
    expect(useOrbit.getState().snapshot.settings.model).toBe(
      "demo-review-model",
    );
  });
  it("resolves only empty workspace values and saves one legacy chat choice without rewriting history", async () => {
    const old = structuredClone(useOrbit.getState().snapshot);
    old.settings.model = "";
    old.settings.effort = "";
    old.reviews[0].model = "codex-default";
    old.reviews[0].effort = "";
    old.reviews[0].confirmedReviewSelection = undefined;
    localStorage.setItem("orbit-demo-v3", JSON.stringify(old));
    await useOrbit.getState().init();
    const store = useOrbit.getState();
    expect(store.snapshot.settings.effort).toBe("high");
    expect(store.snapshot.modelSelectionNotice).toContain("demo-review-model");
    expect(store.snapshot.modelSelectionNotice).toContain(
      "Effort: your Codex configuration.",
    );
    expect(
      JSON.parse(localStorage.getItem("orbit-demo-v3")!).settings.effort,
    ).toBe("high");
    const r = store.snapshot.reviews[0];
    expect(
      (await store.action("send_message", { reviewId: r.id, text: "Explain" }))
        .ok,
    ).toBe(false);
    const selection = {
      reviewId: r.id,
      model: "demo-review-model",
      effort: "low",
    };
    expect((await store.action("set_legacy_chat_model", selection)).ok).toBe(
      true,
    );
    expect((await store.action("set_legacy_chat_model", selection)).ok).toBe(
      true,
    );
    expect(
      (
        await store.action("set_legacy_chat_model", {
          ...selection,
          effort: "high",
        })
      ).ok,
    ).toBe(false);
    expect(
      (await store.action("send_message", { reviewId: r.id, text: "Explain" }))
        .ok,
    ).toBe(true);
    const updated = useOrbit.getState().snapshot.reviews[0];
    expect(updated.confirmedReviewSelection).toBeUndefined();
    expect(updated.model).toBe("codex-default");
    expect(updated.report).toBe(r.report);
    expect(
      updated.messages.find((m) => m.role === "user")?.confirmedSelection,
    ).toEqual({ model: selection.model, effort: "low" });
  });
});

it("preserves unavailable explicit demo settings and permits unrelated edits", async () => {
  const old = structuredClone(useOrbit.getState().snapshot);
  old.settings.model = "no-longer-available";
  old.settings.automatic = true;
  localStorage.setItem("orbit-demo-v3", JSON.stringify(old));
  await useOrbit.getState().init();
  const store = useOrbit.getState();
  expect(store.snapshot.settings.model).toBe("no-longer-available");
  expect(store.snapshot.modelConfigurationError).toContain("unavailable");
  expect((await store.action("start_review", { prId: old.prs[1].id })).ok).toBe(
    false,
  );
  expect((await store.updateSettings({ automatic: false })).ok).toBe(true);
});

it("clears the resolution notice on the next successful demo connection", async () => {
  const old = structuredClone(useOrbit.getState().snapshot);
  old.settings.model = "";
  old.settings.effort = "";
  localStorage.setItem("orbit-demo-v3", JSON.stringify(old));
  await useOrbit.getState().init();
  expect(useOrbit.getState().snapshot.modelSelectionNotice).toContain(
    "Resolved review selection",
  );
  await useOrbit.getState().action("connect_services");
  expect(useOrbit.getState().snapshot.modelSelectionNotice).toBeUndefined();
  expect(
    JSON.parse(localStorage.getItem("orbit-demo-v3")!).modelSelectionNotice,
  ).toBeUndefined();
});

it("does not activate a model pair or legacy choice after a simulated save failure", async () => {
  const old = structuredClone(useOrbit.getState().snapshot);
  old.reviews[0].model = "";
  localStorage.setItem("orbit-demo-v3", JSON.stringify(old));
  await useOrbit.getState().init();
  const save = vi.spyOn(localStorage, "setItem").mockImplementationOnce(() => {
    throw Error("quota exceeded");
  });
  const store = useOrbit.getState();
  expect(
    (await store.updateSettings({ model: "demo-review-model", effort: "low" }))
      .ok,
  ).toBe(false);
  expect(useOrbit.getState().snapshot.settings.effort).toBe("medium");
  save.mockImplementationOnce(() => {
    throw Error("quota exceeded");
  });
  expect(
    (
      await store.action("set_legacy_chat_model", {
        reviewId: old.reviews[0].id,
        model: "demo-review-model",
        effort: "low",
      })
    ).ok,
  ).toBe(false);
  expect(
    useOrbit.getState().snapshot.reviews[0].legacyChatSelection,
  ).toBeUndefined();
  expect(
    (
      await store.action("send_message", {
        reviewId: old.reviews[0].id,
        text: "Explain",
      })
    ).ok,
  ).toBe(false);
});
describe("review loop controls", () => {
  it("stops a loop and continues it with two more rounds", async () => {
    const store = useOrbit.getState();
    const prId = store.snapshot.prs[1].id;
    const loop = () =>
      useOrbit.getState().snapshot.loopStates!.find((l) => l.prId === prId)!;
    expect(loop().status).toBe("needs_you");
    await store.action("continue_loop", { prId, rounds: 2 });
    expect(loop()).toMatchObject({
      status: "waiting_for_author",
      reason: null,
      roundLimit: 6,
    });
    await store.action("stop_loop", { prId });
    expect(loop().status).toBe("stopped");
  });
});
