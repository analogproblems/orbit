// @vitest-environment jsdom
import "./test-setup";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { BrowserNotifications } from "./BrowserNotifications";

let permission: NotificationPermission = "default";
Object.defineProperty(window, "isSecureContext", {
  configurable: true,
  value: true,
});
Object.defineProperty(navigator, "serviceWorker", {
  configurable: true,
  value: {},
});
vi.stubGlobal(
  "Notification",
  Object.assign(
    Object.defineProperty(vi.fn(), "permission", { get: () => permission }),
    {
      requestPermission: vi.fn(async () => {
        permission = "granted";
        return permission;
      }),
    },
  ),
);
afterEach(cleanup);

it("asks this browser to show Orbit's notifications", async () => {
  permission = "default";
  render(<BrowserNotifications />);
  fireEvent.click(
    screen.getByRole("button", { name: "Show them in this browser" }),
  );
  expect(
    await screen.findByText(
      "This browser shows them too, while Orbit’s page is open in it.",
    ),
  ).toBeTruthy();
  expect(Notification.requestPermission).toHaveBeenCalledOnce();
});

it("says when this browser blocks them or needs the https link", () => {
  permission = "denied";
  render(<BrowserNotifications />);
  expect(
    screen.getByText(/This browser blocks Orbit’s notifications/),
  ).toBeTruthy();
  expect(screen.queryByRole("button")).toBeNull();
  cleanup();
  Object.defineProperty(window, "isSecureContext", { value: false });
  render(<BrowserNotifications />);
  expect(
    screen.getByText(/open Orbit’s https link through Tailscale/),
  ).toBeTruthy();
});
