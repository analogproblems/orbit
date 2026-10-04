// @vitest-environment jsdom
import { expect, it, vi } from "vitest";

const token = "cd".repeat(32);
const { fetchMock } = vi.hoisted(() => {
  // The store picks its mode when it loads, from the address it was opened at.
  history.replaceState(null, "", `/#token=${"cd".repeat(32)}`);
  const fetchMock = vi.fn();
  vi.stubGlobal("fetch", fetchMock);
  return { fetchMock };
});
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { desktop, live, remote, useOrbit } from "./store";

it("runs against the desktop app from a browser with an access link", async () => {
  expect([desktop, remote, live]).toEqual([false, true, true]);
  const encoder = new TextEncoder();
  fetchMock.mockImplementation(async (url: string) => {
    if (url === "/api/events")
      return new Response(
        new ReadableStream({
          start(c) {
            c.enqueue(encoder.encode('{"prs":[],"githubUser":"jason"}\n'));
          },
        }),
      );
    if (url === "/api/export_report")
      return Response.json({ fileName: "report.md", text: "# Report" });
    return Response.json("review-1");
  });
  const stop = await useOrbit.getState().init();
  await vi.waitFor(() =>
    expect(useOrbit.getState().snapshot.githubUser).toBe("jason"),
  );
  expect(useOrbit.getState().loaded).toBe(true);

  const started = await useOrbit
    .getState()
    .action<string>("start_review", { prId: "a/b#1" });
  expect(started).toEqual({ ok: true, data: "review-1" });
  const [, init] = fetchMock.mock.calls.find(
    ([url]) => url === "/api/start_review",
  )!;
  expect(init.headers.authorization).toBe(`Bearer ${token}`);
  expect(JSON.parse(init.body)).toEqual({ prId: "a/b#1" });

  const createObjectURL = vi.fn(() => "blob:report");
  Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() });
  const click = vi
    .spyOn(HTMLAnchorElement.prototype, "click")
    .mockImplementation(() => {});
  const exported = await useOrbit
    .getState()
    .action<string>("export_report", { reviewId: "review-1" });
  expect(exported).toEqual({ ok: true, data: "report.md" });
  expect(click).toHaveBeenCalled();
  stop();
});
