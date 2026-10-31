import { expect, it, vi } from "vitest";
import type { Snapshot } from "./types";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: invokeMock,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
import { useOrbit } from "./store";

const lost = "Lost connection to the always-on Orbit. Reconnecting…";

it("sends everything to the always-on Orbit it shows, but opening links", async () => {
  const handlers = new Map<string, (event: { payload: unknown }) => void>();
  listenMock.mockImplementation(async (name: string, handler) => {
    handlers.set(name, handler);
    return () => handlers.delete(name);
  });
  // It can't be reached at first.
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_always_on")
      return { host: "box.example.ts.net", chosen: true };
    throw "Orbit can't reach the always-on Orbit at box.example.ts.net.";
  });
  const stop = await useOrbit.getState().init();
  expect(invokeMock).toHaveBeenCalledWith("forward", {
    command: "get_snapshot",
    args: {},
  });
  expect(invokeMock).not.toHaveBeenCalledWith("get_snapshot");
  expect(useOrbit.getState()).toMatchObject({
    alwaysOn: { host: "box.example.ts.net", chosen: true },
    loaded: true,
    error: "Orbit can't reach the always-on Orbit at box.example.ts.net.",
  });
  // Then it answers: its snapshot shows, and the problem goes.
  const state = handlers.get("orbit-state")!;
  const connection = handlers.get("orbit-connection")!;
  state({ payload: { prs: [], githubUser: "there" } as Partial<Snapshot> });
  connection({ payload: null });
  expect(useOrbit.getState().snapshot.githubUser).toBe("there");
  expect(useOrbit.getState().error).toBeNull();
  // A lost connection shows until it's back, and leaves other errors be.
  connection({ payload: lost });
  expect(useOrbit.getState().error).toBe(lost);
  connection({ payload: null });
  expect(useOrbit.getState().error).toBeNull();
  useOrbit.setState({ error: "Something else went wrong." });
  connection({ payload: null });
  expect(useOrbit.getState().error).toBe("Something else went wrong.");

  invokeMock.mockReset();
  invokeMock.mockResolvedValue("review-1");
  const started = await useOrbit
    .getState()
    .action<string>("start_review", { prId: "a/b#1" });
  expect(started).toEqual({ ok: true, data: "review-1" });
  expect(invokeMock).toHaveBeenLastCalledWith("forward", {
    command: "start_review",
    args: { prId: "a/b#1" },
  });
  await useOrbit
    .getState()
    .action("open_github", { url: "https://github.com/a/b" });
  expect(invokeMock).toHaveBeenLastCalledWith("open_github", {
    url: "https://github.com/a/b",
  });
  await useOrbit.getState().action("stop_always_on");
  expect(invokeMock).toHaveBeenLastCalledWith("stop_always_on", {});
  stop();
  expect(handlers.size).toBe(0);
});
