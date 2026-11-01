// Notifications in a browser using an Orbit (see remote.ts): each one the
// Orbit sends (`Snapshot.notices`) shows here once, as it does in a desktop
// app showing an always-on Orbit (`client::Relay` in client.rs), once you
// turn them on in this browser. They show through Orbit's service worker
// (public/sw.js), as phones need, and a tap opens the item. A browser Orbit
// wakes with its page closed (push.ts) gets them from the worker instead.
import { wokenHere } from "./push";
import type { Attention, SentNotice, Snapshot } from "./types";

const seenKey = "orbit-notices-seen";
/** How long a notification waits for Orbit's service worker to start, the
 * first time, before showing without it. */
const starting = 10_000;

/** What this browser does with Orbit's notifications. */
export type BrowserNotices =
  "on" | "off" | "blocked" | "needsHttps" | "needsHomeScreen" | "unsupported";

/** Whether this browser shows Orbit's notifications, and if not, why. */
export function browserNotices(): BrowserNotices {
  if (typeof window === "undefined") return "unsupported";
  if (!window.isSecureContext) return "needsHttps";
  if (!("Notification" in window) || !("serviceWorker" in navigator)) {
    // Apple's phones and tablets show them only for a page added to the
    // Home Screen and opened from there.
    return appleTouch() ? "needsHomeScreen" : "unsupported";
  }
  switch (Notification.permission) {
    case "granted":
      return "on";
    case "denied":
      return "blocked";
    default:
      return "off";
  }
}
function appleTouch() {
  const agent = navigator.userAgent;
  return (
    /iPhone|iPad|iPod/.test(agent) ||
    (agent.includes("Macintosh") && navigator.maxTouchPoints > 1)
  );
}

/** Asks this browser to show Orbit's notifications, after a tap. */
export async function turnOnBrowserNotices(): Promise<BrowserNotices> {
  await Notification.requestPermission();
  return browserNotices();
}

let worker: Promise<ServiceWorkerRegistration | null> | null = null;
/** Starts Orbit's service worker, and has a tap on a notification open its
 * item with `open`. Returns what stops listening for taps. */
export function startBrowserNotices(open: (item: Attention) => void) {
  if (
    typeof window === "undefined" ||
    !window.isSecureContext ||
    !("serviceWorker" in navigator)
  ) {
    return () => {};
  }
  worker = navigator.serviceWorker
    .register("/sw.js")
    // Showing one needs an active worker, and installing comes first.
    .then(() => navigator.serviceWorker.ready)
    .catch(() => null);
  const tapped = (e: MessageEvent) => {
    const data = e.data as { type?: string; item?: Attention } | null;
    if (data?.type === "orbit-open" && data.item) open(data.item);
  };
  navigator.serviceWorker.addEventListener("message", tapped);
  return () => navigator.serviceWorker.removeEventListener("message", tapped);
}

/** The notifications to show for `snapshot`, given those already `seen`
 * here, and what to remember as seen: those not shown before whose item
 * still needs you, while notifications are on. With nothing seen yet,
 * what's there only becomes seen, as it was sent before this browser
 * followed that Orbit. */
export function pick(
  seen: string[] | null,
  snapshot: Snapshot,
): { fresh: SentNotice[]; seen: string[] } {
  const sent = snapshot.notices ?? [];
  const ids = sent.map((n) => n.id);
  if (seen === null) return { fresh: [], seen: ids };
  const known = new Set(seen);
  const open = new Set((snapshot.attention ?? []).map((a) => a.key));
  // As sent: a snapshot from an older Orbit may leave settings out.
  const on =
    (snapshot.settings as Snapshot["settings"] | undefined)?.notify ?? true;
  const fresh = sent.filter(
    (n) => on && !known.has(n.id) && open.has(n.item.key),
  );
  return { fresh, seen: ids };
}

/** Shows the notifications new in `snapshot`, if this browser shows them
 * and Orbit doesn't wake it for them. What's seen is remembered either
 * way, so turning them on later shows nothing old. */
export function relayNotices(snapshot: Snapshot) {
  try {
    const seen = load();
    const picked = pick(seen, snapshot);
    if (seen === null || picked.seen.join() !== seen.join()) save(picked.seen);
    if (picked.fresh.length === 0 || browserNotices() !== "on") return;
    if (wokenHere(snapshot)) return;
    for (const n of picked.fresh) void show(n);
  } catch {
    // Never holds up the page: Needs you still lists every item.
  }
}
async function show(n: SentNotice) {
  // The same tag for the same notification, so two open pages show it once.
  const options: NotificationOptions = {
    body: n.body,
    tag: n.id,
    data: { item: n.item },
  };
  try {
    const registration = worker
      ? await Promise.race([
          worker,
          new Promise<null>((r) => setTimeout(() => r(null), starting)),
        ])
      : null;
    if (registration) await registration.showNotification(n.title, options);
    else new Notification(n.title, options);
  } catch {
    // A browser that won't show it now still lists the item in Needs you.
  }
}
// Also kept here, for a browser that refuses storage, such as in private
// browsing: that visit still shows each once.
let kept: string[] | null = null;
function load(): string[] | null {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(seenKey) ?? "null");
    if (Array.isArray(saved)) return saved.map(String);
  } catch {
    // Read from memory instead.
  }
  return kept;
}
function save(ids: string[]) {
  kept = ids;
  try {
    localStorage.setItem(seenKey, JSON.stringify(ids));
  } catch {
    // Kept in memory only.
  }
}
