// Orbit's service worker (public/sw.js), run with a stand-in for the
// browser around it.
/// <reference types="vite/client" />
import source from "../public/sw.js?raw";
import { beforeEach, expect, it, vi } from "vitest";

type Listener = (event: unknown) => void;
interface Shown {
  title: string;
  options: Record<string, unknown>;
}
let listeners: Record<string, Listener>;
let shown: Shown[];
let showing: Shown[];
let saved: string | null;
let deleted: string[];
const unsubscribe = vi.fn(async () => true);
const fetch = vi.fn();
/** The worker's Web Locks, where a browser has them. */
let locks: { request: ReturnType<typeof vi.fn> } | undefined;

beforeEach(() => {
  listeners = {};
  shown = [];
  showing = [];
  saved = JSON.stringify({ id: "phone", key: "k".repeat(64) });
  deleted = [];
  unsubscribe.mockClear();
  fetch.mockReset();
  locks = undefined;
  const self = {
    get navigator() {
      return { locks };
    },
    addEventListener: (type: string, f: Listener) => (listeners[type] = f),
    registration: {
      showNotification: async (
        title: string,
        options: Record<string, unknown>,
      ) => void shown.push({ title, options }),
      getNotifications: async () =>
        showing.map((n) => ({ title: n.title, ...n.options })),
      pushManager: { getSubscription: async () => ({ unsubscribe }) },
    },
    clients: {},
    skipWaiting: () => {},
  };
  const caches = {
    open: async () => ({
      match: async () => (saved === null ? undefined : new Response(saved)),
    }),
    delete: async (name: string) => void deleted.push(name),
  };
  new Function("self", "caches", "fetch", source)(self, caches, fetch);
});
/** A wake-up from Orbit, and what it shows once handled. */
async function wake() {
  let handled: Promise<unknown> = Promise.resolve();
  listeners.push({ waitUntil: (p: Promise<unknown>) => (handled = p) });
  await handled;
  return shown;
}
const notice = (id: string) => ({
  id,
  seq: 1,
  title: `Title ${id}`,
  body: `Body ${id}`,
  item: { key: `approval:${id}` },
});
const answer = (status: number, body: unknown) =>
  fetch.mockResolvedValueOnce(new Response(JSON.stringify(body), { status }));

it("reads what's new from Orbit with this device's key and shows each", async () => {
  answer(200, { notices: [notice("a"), notice("b")] });
  expect(await wake()).toEqual([
    {
      title: "Title a",
      options: {
        body: "Body a",
        tag: "a",
        data: { item: { key: "approval:a" } },
      },
    },
    {
      title: "Title b",
      options: {
        body: "Body b",
        tag: "b",
        data: { item: { key: "approval:b" } },
      },
    },
  ]);
  expect(fetch).toHaveBeenCalledWith("/push/notices", {
    method: "POST",
    headers: { Authorization: `Bearer ${"k".repeat(64)}` },
    cache: "no-store",
    signal: expect.any(AbortSignal),
  });
});

it("shows something for every wake-up, as browsers ask", async () => {
  // Already read: the latest showing is shown again, quietly.
  showing = [
    { title: "Title a", options: { body: "Body a", tag: "a", data: {} } },
  ];
  answer(200, { notices: [] });
  expect(await wake()).toEqual([
    {
      title: "Title a",
      options: { body: "Body a", tag: "a", data: {}, silent: true },
    },
  ]);
  shown = [];
  showing = [];
  answer(200, { notices: [] });
  expect(await wake()).toEqual([
    {
      title: "Orbit",
      options: {
        body: "Nothing new needs you now.",
        tag: "orbit-wake",
        data: undefined,
      },
    },
  ]);
  // Orbit out of reach, or answering oddly.
  for (const fails of [
    () => fetch.mockRejectedValueOnce(TypeError("Failed to fetch")),
    () => answer(500, { error: "x" }),
    () => (saved = null),
  ]) {
    shown = [];
    fails();
    expect((await wake()).map((n) => [n.title, n.options.body])).toEqual([
      [
        "Something needs you",
        "Orbit couldn’t be reached to say what. Open Orbit to see it.",
      ],
    ]);
  }
});

it("signs out of wake-ups once Orbit no longer wakes this device", async () => {
  answer(401, {
    error:
      "This device no longer gets Orbit's alerts. Turn them on again in Settings, under Notifications.",
  });
  expect((await wake()).map((n) => [n.title, n.options.body])).toEqual([
    [
      "Orbit",
      "This device no longer gets Orbit's alerts. Turn them on again in Settings, under Notifications.",
    ],
  ]);
  expect(deleted).toEqual(["orbit-push"]);
  expect(unsubscribe).toHaveBeenCalledOnce();
});

it("reads only once a page signing this browser up has kept its key", async () => {
  let signedUp: () => void = () => {};
  const signingUp = new Promise<void>((r) => (signedUp = r));
  locks = {
    request: vi.fn(async (_: string, work: () => Promise<unknown>) => {
      await signingUp;
      return work();
    }),
  };
  answer(200, { notices: [notice("a")] });
  const woken = wake();
  await new Promise((r) => setTimeout(r, 10));
  expect(fetch).not.toHaveBeenCalled();
  // The page keeps its new key, then lets go.
  saved = JSON.stringify({ id: "phone", key: "n".repeat(64) });
  signedUp();
  expect((await woken).map((n) => n.title)).toEqual(["Title a"]);
  expect(locks.request.mock.calls[0][0]).toBe("orbit-push");
  expect(fetch.mock.calls[0][1].headers.Authorization).toBe(
    `Bearer ${"n".repeat(64)}`,
  );
});
