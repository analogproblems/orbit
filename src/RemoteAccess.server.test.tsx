// @vitest-environment jsdom
import "./test-setup";
import { expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.hoisted(() => {
  // Opened from orbit-server, which marks its page as served without a window.
  const mark = document.createElement("meta");
  mark.name = "orbit-served";
  mark.content = "server";
  document.head.append(mark);
  history.replaceState(null, "", `/#token=${"ef".repeat(32)}`);
  vi.stubGlobal("fetch", vi.fn());
});
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { RemoteAccess } from "./RemoteAccess";
import { desktop, live, remote, windowless } from "./store";

it("says a windowless Orbit's link is made on the computer it runs on", () => {
  expect([desktop, remote, live, windowless]).toEqual([
    false,
    true,
    true,
    true,
  ]);
  render(<RemoteAccess />);
  expect(
    screen.getByText(
      "You’re using an Orbit that runs without a window. To make a new link, stop it on the computer it runs on, run orbit-server link --new there, then start it again.",
    ),
  ).toBeTruthy();
  expect(screen.queryByText(/from the desktop app/)).toBeNull();
  expect(screen.queryByRole("switch")).toBeNull();
});
