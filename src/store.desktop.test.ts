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
  invokeMock.mockReturnValue(new Promise((resolve) => (answer = resolve)));
  const started = useOrbit.getState().init();
  await vi.waitFor(() => expect(invokeMock).toHaveBeenCalled());
  send({ payload: { prs: [], githubUser: "newer" } });
  answer({ prs: [], githubUser: "older" });
  await started;
  expect(useOrbit.getState().snapshot.githubUser).toBe("newer");
});
