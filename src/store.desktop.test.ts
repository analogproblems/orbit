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

it("keeps a change that arrives before the first snapshot", async () => {
  let send: (event: { payload: Partial<Snapshot> }) => void = () => {};
  listenMock.mockImplementation(async (_name, handler) => {
    send = handler;
    return () => {};
  });
  let answer: (s: Partial<Snapshot>) => void = () => {};
  const first = new Promise((resolve) => (answer = resolve));
  // This app runs its own Orbit.
  invokeMock.mockImplementation(async (command: string) =>
    command === "get_always_on" ? null : first,
  );
  const started = useOrbit.getState().init();
  await vi.waitFor(() =>
    expect(invokeMock).toHaveBeenCalledWith("get_snapshot"),
  );
  send({ payload: { prs: [], githubUser: "newer" } });
  answer({ prs: [], githubUser: "older" });
  await started;
  expect(useOrbit.getState().snapshot.githubUser).toBe("newer");
  expect(useOrbit.getState().alwaysOn).toBeNull();
  // Its actions run here.
  invokeMock.mockResolvedValue("review-1");
  await useOrbit.getState().action("start_review", { prId: "a/b#1" });
  expect(invokeMock).toHaveBeenLastCalledWith("start_review", {
    prId: "a/b#1",
  });
});
