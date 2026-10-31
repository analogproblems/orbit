// @vitest-environment jsdom
import "./test-setup";
import { createElement } from "react";
import { expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

const { fetchMock } = vi.hoisted(() => {
  // Opened from Orbit's own server, which marks its page, with no link.
  const mark = document.createElement("meta");
  mark.name = "orbit-served";
  document.head.append(mark);
  const fetchMock = vi.fn();
  vi.stubGlobal("fetch", fetchMock);
  return { fetchMock };
});
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { expiredLink } from "./remote";
import { RemoteAccess } from "./RemoteAccess";
import { desktop, live, remote, useOrbit, windowless } from "./store";

it("says an old or missing link no longer works instead of showing the demo", async () => {
  expect([desktop, remote, live]).toEqual([false, true, true]);
  // The desktop app's own server marks its page with no content.
  expect(windowless).toBe(false);
  await useOrbit.getState().init();
  const state = useOrbit.getState();
  expect(state.error).toBe(expiredLink);
  expect(state.loaded).toBe(true);
  expect(state.snapshot.prs).toEqual([]);
  expect(
    await useOrbit.getState().action("start_review", { prId: "a/b#1" }),
  ).toEqual({ ok: false, error: `Error: ${expiredLink}` });
  expect(fetchMock).not.toHaveBeenCalled();
});

it("says the desktop app's link is made in the desktop app", () => {
  render(createElement(RemoteAccess));
  expect(
    screen.getByText(
      "You’re using Orbit from another device. Turn this off or make a new link from the desktop app.",
    ),
  ).toBeTruthy();
  expect(screen.queryByText(/without a window/)).toBeNull();
});
