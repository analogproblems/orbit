// @vitest-environment jsdom
import "./test-setup";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("./store", async (original) => ({
  ...(await original<typeof import("./store")>()),
  // A browser using an Orbit through its link.
  remote: true,
}));
import { BrowserNotifications, PushDevices } from "./BrowserNotifications";
import { deviceName } from "./push";
import { useOrbit } from "./store";
import type { PushDevice } from "./types";

/** This Orbit's key, as push.rs gives it: an uncompressed point. */
const raw = Uint8Array.from({ length: 65 }, (_, i) => (i === 0 ? 4 : i));
const key = btoa(String.fromCharCode(...raw))
  .replace(/\+/g, "-")
  .replace(/\//g, "_")
  .replace(/=+$/, "");
const deviceKey = "d".repeat(64);
const endpoint = "https://fcm.googleapis.com/fcm/send/abc";

interface SignedUp {
  endpoint: string;
  options: { applicationServerKey: ArrayBuffer };
  unsubscribe: ReturnType<typeof vi.fn>;
}
let signedUp: SignedUp | null;
const signUp = (server: Uint8Array): SignedUp => ({
  endpoint,
  options: { applicationServerKey: server.slice().buffer },
  unsubscribe: vi.fn(async () => {
    signedUp = null;
    return true;
  }),
});
const pushManager = {
  getSubscription: vi.fn(async () => signedUp),
  subscribe: vi.fn(
    async (o: { applicationServerKey: Uint8Array }) =>
      (signedUp = signUp(o.applicationServerKey)),
  ),
};
/** Cache Storage, as the service worker reads it. */
const cached = new Map<string, Map<string, string>>();
const action = vi.fn();
const devices = (list: PushDevice[]) =>
  useOrbit.setState((s) => ({
    snapshot: { ...s.snapshot, pushDevices: list },
  }));

beforeEach(() => {
  signedUp = null;
  cached.clear();
  localStorage.clear();
  pushManager.subscribe.mockClear();
  Object.defineProperty(window, "isSecureContext", {
    configurable: true,
    value: true,
  });
  Object.defineProperty(navigator, "serviceWorker", {
    configurable: true,
    value: { ready: Promise.resolve({ pushManager }) },
  });
  vi.stubGlobal("PushManager", class {});
  vi.stubGlobal(
    "Notification",
    Object.defineProperty(vi.fn(), "permission", { get: () => "granted" }),
  );
  vi.stubGlobal("caches", {
    open: async (name: string) => {
      const cache = cached.get(name) ?? new Map<string, string>();
      cached.set(name, cache);
      return {
        put: async (url: string, response: Response) =>
          void cache.set(url, await response.text()),
      };
    },
    delete: async (name: string) => cached.delete(name),
  });
  action.mockReset();
  action.mockImplementation(
    async (command: string, args: Record<string, unknown>) => {
      if (command === "push_key") return { ok: true, data: key };
      if (command === "add_push_device") {
        devices([
          {
            id: "phone",
            name: String(args.name),
            added: "2026-11-01T09:00:00Z",
          },
        ]);
        return { ok: true, data: { id: "phone", key: deviceKey } };
      }
      if (command === "remove_push_device") {
        devices([]);
        return { ok: true, data: undefined };
      }
      throw Error(command);
    },
  );
  useOrbit.setState({ action: action as never });
  devices([]);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
const closed = () =>
  screen.getByRole("switch", { name: /Also when Orbit’s page is closed/ });

it("has Orbit wake this browser with its page closed, then stops", async () => {
  render(
    <>
      <BrowserNotifications />
      <PushDevices />
    </>,
  );
  expect((closed() as HTMLInputElement).checked).toBe(false);
  await userEvent.click(closed());
  await waitFor(() =>
    expect((closed() as HTMLInputElement).checked).toBe(true),
  );
  // Signed up with this Orbit's key, for wake-ups it always shows.
  const options = pushManager.subscribe.mock.calls[0][0] as {
    userVisibleOnly: boolean;
    applicationServerKey: Uint8Array;
  };
  expect(options.userVisibleOnly).toBe(true);
  expect([...options.applicationServerKey]).toEqual([...raw]);
  expect(action).toHaveBeenCalledWith(
    "add_push_device",
    { endpoint, name: deviceName() },
    { quiet: true },
  );
  // Kept where the service worker reads it, and the id for the page.
  expect(
    JSON.parse(cached.get("orbit-push")?.get("/orbit-push-device") ?? ""),
  ).toEqual({ id: "phone", key: deviceKey });
  expect(localStorage.getItem("orbit-push-device")).toBe("phone");
  expect(screen.getByText(`${deviceName()} (this browser)`)).toBeTruthy();

  const signed = signedUp;
  await userEvent.click(closed());
  await waitFor(() =>
    expect((closed() as HTMLInputElement).checked).toBe(false),
  );
  expect(action).toHaveBeenCalledWith(
    "remove_push_device",
    { id: "phone" },
    { quiet: true },
  );
  expect(signed?.unsubscribe).toHaveBeenCalled();
  expect(cached.has("orbit-push")).toBe(false);
  expect(localStorage.getItem("orbit-push-device")).toBeNull();
  expect(screen.queryByText(`${deviceName()} (this browser)`)).toBeNull();
});

it("signs up again when this browser was signed up with another key", async () => {
  const old = signUp(new Uint8Array(65).fill(9));
  signedUp = old;
  render(<BrowserNotifications />);
  await userEvent.click(closed());
  await waitFor(() =>
    expect((closed() as HTMLInputElement).checked).toBe(true),
  );
  expect(old.unsubscribe).toHaveBeenCalled();
  expect(pushManager.subscribe).toHaveBeenCalledOnce();
  // One signed up with this key already is used as it is.
  cleanup();
  devices([]);
  render(<BrowserNotifications />);
  await userEvent.click(closed());
  await waitFor(() =>
    expect((closed() as HTMLInputElement).checked).toBe(true),
  );
  expect(pushManager.subscribe).toHaveBeenCalledOnce();
});

it("says why this browser couldn't be woken", async () => {
  pushManager.subscribe.mockRejectedValueOnce(Error("NotAllowedError"));
  render(<BrowserNotifications />);
  await userEvent.click(closed());
  expect(
    await screen.findByText(
      "This browser wouldn’t sign up for Orbit’s wake-ups. Check that it allows Orbit’s notifications, then try again.",
    ),
  ).toBeTruthy();
  expect((closed() as HTMLInputElement).checked).toBe(false);
  // Orbit refusing it signs this browser out again.
  action.mockImplementation(async (command: string) =>
    command === "push_key"
      ? { ok: true, data: key }
      : {
          ok: false,
          error:
            "This browser's push service isn't one Orbit knows, so Orbit can't wake it.",
        },
  );
  await userEvent.click(closed());
  expect(
    await screen.findByText(
      "This browser's push service isn't one Orbit knows, so Orbit can't wake it.",
    ),
  ).toBeTruthy();
  expect(signedUp).toBeNull();
  expect(cached.has("orbit-push")).toBe(false);
});

it("says when this browser can't be woken at all", () => {
  // Not there at all, as in some private windows.
  delete (window as { PushManager?: unknown }).PushManager;
  render(<BrowserNotifications />);
  expect(
    screen.getByText(
      "This browser can’t get them while Orbit’s page is closed.",
    ),
  ).toBeTruthy();
});

it("lists each browser Orbit wakes, with why one failed, and removes one", async () => {
  devices([
    { id: "phone", name: "iPhone", added: "2026-11-01T09:00:00Z" },
    {
      id: "laptop",
      name: "Firefox on Windows",
      added: "2026-11-01T10:00:00Z",
      error: "Orbit couldn't wake it: Its push service answered 500.",
    },
  ]);
  render(<PushDevices />);
  expect(screen.getByText("iPhone")).toBeTruthy();
  expect(
    screen.getByText("Orbit couldn't wake it: Its push service answered 500."),
  ).toBeTruthy();
  await userEvent.click(screen.getAllByRole("button", { name: "Remove" })[1]);
  expect(action).toHaveBeenCalledWith(
    "remove_push_device",
    { id: "laptop" },
    { quiet: true },
  );
  // With none left, there's no list.
  await waitFor(() => expect(screen.queryByText("iPhone")).toBeNull());
});

it("names a browser plainly", () => {
  const named = (agent: string) => deviceName(agent);
  expect(
    named(
      "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1",
    ),
  ).toBe("iPhone");
  expect(
    named(
      "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
    ),
  ).toBe("Android");
  expect(
    named(
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
    ),
  ).toBe("Edge on Windows");
  expect(
    named(
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 14.6; rv:132.0) Gecko/20100101 Firefox/132.0",
    ),
  ).toBe("Firefox on Mac");
  expect(
    named(
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_6) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15",
    ),
  ).toBe("Safari on Mac");
  expect(named("Something else")).toBe("A browser");
});
