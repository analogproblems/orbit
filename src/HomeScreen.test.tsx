// @vitest-environment jsdom
import "./test-setup";
import { expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const token = "7c".repeat(32);
vi.hoisted(() => {
  // Safari on an iPhone, opened from Orbit's access link: no notifications
  // until Orbit is on the Home Screen.
  history.replaceState(null, "", `/#token=${"7c".repeat(32)}`);
  Object.defineProperty(window, "isSecureContext", { value: true });
  Object.defineProperty(navigator, "userAgent", {
    value:
      "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1",
  });
  vi.stubGlobal("fetch", vi.fn());
});
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { BrowserNotifications } from "./BrowserNotifications";

it("copies the access link for Orbit on the Home Screen to ask for", async () => {
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
  render(<BrowserNotifications />);
  expect(
    screen.getByText(
      "To show them on an iPhone or iPad, add this page to the Home Screen (Share, then Add to Home Screen), then open Orbit from there. It asks once for Orbit’s access link, which you can copy here.",
    ),
  ).toBeTruthy();
  await userEvent.click(
    screen.getByRole("button", { name: "Copy the access link" }),
  );
  expect(writeText).toHaveBeenCalledWith(`${location.origin}/#token=${token}`);
  expect(
    await screen.findByText(
      "Copied. Paste it when Orbit on the Home Screen asks for it.",
    ),
  ).toBeTruthy();
  // A browser that won't copy says what to paste instead.
  writeText.mockRejectedValueOnce(Error("not allowed"));
  await userEvent.click(
    screen.getByRole("button", { name: "Copy the access link" }),
  );
  expect(
    await screen.findByText(
      "This browser wouldn’t copy it. Paste the link you opened Orbit with instead.",
    ),
  ).toBeTruthy();
});
