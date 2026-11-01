// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Attention, SentNotice, Snapshot } from "./types";

const shown = vi.fn();
let permission: NotificationPermission = "granted";
const tapped: ((e: MessageEvent) => void)[] = [];
beforeEach(() => {
  localStorage.clear();
  shown.mockReset();
  permission = "granted";
  tapped.length = 0;
  Object.defineProperty(window, "isSecureContext", {
    configurable: true,
    value: true,
  });
  const fake = Object.defineProperty(vi.fn(), "permission", {
    get: () => permission,
  });
  vi.stubGlobal(
    "Notification",
    Object.assign(fake, {
      requestPermission: vi.fn(async () => {
        permission = "granted";
        return permission;
      }),
    }),
  );
  Object.defineProperty(navigator, "serviceWorker", {
    configurable: true,
    value: {
      register: vi.fn(async () => ({ showNotification: shown })),
      addEventListener: (_: string, f: (e: MessageEvent) => void) =>
        tapped.push(f),
      removeEventListener: vi.fn(),
    },
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

const item = (key: string): Attention =>
  ({ key, kind: "approval", title: key }) as unknown as Attention;
const notice = (id: string, key: string): SentNotice => ({
  id,
  title: `Title ${id}`,
  body: `Body ${id}`,
  item: item(key),
});
const snapshot = (
  notices: SentNotice[],
  open: string[],
  notify = true,
): Snapshot =>
  ({
    notices,
    attention: open.map(item),
    settings: { notify },
  }) as unknown as Snapshot;
const flush = () => new Promise((r) => setTimeout(r, 0));

it("shows one notification for a new Needs you item, once", async () => {
  const n = await import("./notices");
  const open = vi.fn();
  n.startBrowserNotices(open);
  // What was there before this browser followed the Orbit is only seen.
  n.relayNotices(snapshot([notice("a", "approval:1")], ["approval:1"]));
  await flush();
  expect(shown).not.toHaveBeenCalled();

  const later = [notice("a", "approval:1"), notice("b", "held:2")];
  n.relayNotices(snapshot(later, ["approval:1", "held:2"]));
  n.relayNotices(snapshot(later, ["approval:1", "held:2"]));
  await flush();
  expect(shown).toHaveBeenCalledTimes(1);
  expect(shown).toHaveBeenCalledWith("Title b", {
    body: "Body b",
    tag: "b",
    data: { item: item("held:2") },
  });
  // A tap on it opens its item.
  tapped[0]({ data: { type: "orbit-open", item: item("held:2") } } as never);
  expect(open).toHaveBeenCalledWith(item("held:2"));
});

it("shows nothing for an item already closed, or with notifications off", async () => {
  const n = await import("./notices");
  n.startBrowserNotices(() => {});
  n.relayNotices(snapshot([], []));
  n.relayNotices(snapshot([notice("a", "approval:1")], []));
  n.relayNotices(
    snapshot(
      [notice("a", "approval:1"), notice("b", "held:2")],
      ["held:2"],
      false,
    ),
  );
  await flush();
  expect(shown).not.toHaveBeenCalled();
  // Nor, once a browser turns them on, any it saw before.
  n.relayNotices(
    snapshot([notice("a", "approval:1"), notice("b", "held:2")], ["held:2"]),
  );
  await flush();
  expect(shown).not.toHaveBeenCalled();
});

it("remembers what it saw while this browser didn't show them", async () => {
  permission = "default";
  const n = await import("./notices");
  n.startBrowserNotices(() => {});
  n.relayNotices(snapshot([], []));
  n.relayNotices(snapshot([notice("a", "approval:1")], ["approval:1"]));
  await flush();
  expect(shown).not.toHaveBeenCalled();
  expect(await n.turnOnBrowserNotices()).toBe("on");
  n.relayNotices(snapshot([notice("a", "approval:1")], ["approval:1"]));
  await flush();
  expect(shown).not.toHaveBeenCalled();
});

it("says what this browser does with them", async () => {
  const { browserNotices } = await import("./notices");
  expect(browserNotices()).toBe("on");
  permission = "denied";
  expect(browserNotices()).toBe("blocked");
  permission = "default";
  expect(browserNotices()).toBe("off");
  Object.defineProperty(window, "isSecureContext", { value: false });
  expect(browserNotices()).toBe("needsHttps");
});
