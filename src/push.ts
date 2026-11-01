// Orbit's notifications on this browser with its page closed (see push.rs).
// Orbit sends an empty wake-up through the browser's push service, and
// Orbit's service worker (public/sw.js) then reads what's new from Orbit
// itself, with this device's own key, kept where the worker can read it.
// While this is on, the worker shows every notification here, so the page
// doesn't (`relayNotices` in notices.ts).
import { useSyncExternalStore } from "react";
import type { Snapshot } from "./types";

/** Where the service worker finds this device's id and key. */
const cacheName = "orbit-push";
const entry = "/orbit-push-device";
/** Held while this browser is signed up or out, by its pages and its
 * worker alike. */
const lockName = "orbit-push";
/** This device's id, for the page, which reads it at once. */
const idKey = "orbit-push-device";
/** How long turning this on waits for Orbit's service worker to start. */
const starting = 10_000;

/** A device Orbit wakes, as Orbit gives it back once: its id, and the key
 * its service worker reads new notifications with. */
export interface AddedDevice {
  id: string;
  key: string;
}
type Action = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<{ ok: true; data: T } | { ok: false; error: string }>;

/** Whether this browser can be woken with Orbit's page closed. */
export function pushSupported(): boolean {
  return (
    typeof window !== "undefined" &&
    window.isSecureContext &&
    "serviceWorker" in navigator &&
    "PushManager" in window &&
    "caches" in window &&
    "locks" in navigator
  );
}

// Kept here instead, for a browser that refuses storage.
let kept: string | null = null;
/** The id Orbit gave this browser when it was last turned on here. */
export function savedDevice(): string | null {
  try {
    return localStorage.getItem(idKey) ?? kept;
  } catch {
    return kept;
  }
}
const listeners = new Set<() => void>();
/** `savedDevice`, shown again as it changes. */
export function useSavedDevice(): string | null {
  return useSyncExternalStore((changed) => {
    listeners.add(changed);
    // Turned on or off in another page of this Orbit.
    window.addEventListener("storage", changed);
    return () => {
      listeners.delete(changed);
      window.removeEventListener("storage", changed);
    };
  }, savedDevice);
}
/** Whether `snapshot`'s Orbit still wakes this browser. */
export function wokenHere(snapshot: Snapshot): boolean {
  const id = savedDevice();
  return id !== null && (snapshot.pushDevices ?? []).some((d) => d.id === id);
}

/** What this browser calls itself, such as "iPhone" or "Firefox on
 * Windows". */
export function deviceName(agent = navigator.userAgent): string {
  if (/iPhone/.test(agent)) return "iPhone";
  if (
    /iPad/.test(agent) ||
    (agent.includes("Macintosh") && navigator.maxTouchPoints > 1)
  ) {
    return "iPad";
  }
  if (/Android/.test(agent)) return "Android";
  const system = /Windows/.test(agent)
    ? "Windows"
    : /Macintosh/.test(agent)
      ? "Mac"
      : /CrOS/.test(agent)
        ? "Chromebook"
        : /Linux/.test(agent)
          ? "Linux"
          : null;
  const browser = /Edg\//.test(agent)
    ? "Edge"
    : /Firefox\//.test(agent)
      ? "Firefox"
      : /Chrome\//.test(agent)
        ? "Chrome"
        : /Safari\//.test(agent)
          ? "Safari"
          : null;
  if (system && browser) return `${browser} on ${system}`;
  return system ?? browser ?? "A browser";
}

async function worker(): Promise<ServiceWorkerRegistration> {
  const ready = await Promise.race([
    navigator.serviceWorker.ready,
    new Promise<null>((r) => setTimeout(() => r(null), starting)),
  ]);
  if (!ready) throw Error("Orbit’s page couldn’t start here. Reload it.");
  return ready;
}
/** The key a push service checks wake-ups with, as browsers take it. */
function bytes(base64url: string): Uint8Array<ArrayBuffer> {
  const base64 = base64url.replace(/-/g, "+").replace(/_/g, "/");
  const text = atob(base64 + "=".repeat((4 - (base64.length % 4)) % 4));
  return Uint8Array.from(text, (c) => c.charCodeAt(0));
}
function same(a: ArrayBuffer | null | undefined, b: Uint8Array) {
  if (!a || a.byteLength !== b.byteLength) return false;
  const left = new Uint8Array(a);
  return left.every((byte, i) => byte === b[i]);
}

/** Runs `work` while no other page of this Orbit in this browser, nor its
 * worker, signs this browser up or out, as they share one sign-up and one
 * key. */
function alone<T>(work: () => Promise<T>): Promise<T> {
  // Its answer is `work`'s, which DOM types wrap in a second promise.
  return navigator.locks.request(lockName, work) as unknown as Promise<T>;
}

/** Has Orbit wake this browser when it sends a notification, even with
 * its page closed, after a tap. Notifications must be allowed already. */
export function turnOnPush(action: Action): Promise<void> {
  return alone(async () => {
    const registration = await worker();
    const key = await action<string>("push_key");
    if (!key.ok) throw Error(key.error);
    const server = bytes(key.data);
    let signedUp = await registration.pushManager.getSubscription();
    // One made for another Orbit, or an older key, can't take these
    // wake-ups.
    if (signedUp && !same(signedUp.options.applicationServerKey, server)) {
      await signedUp.unsubscribe();
      signedUp = null;
    }
    const made = !signedUp;
    try {
      signedUp ??= await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: server,
      });
    } catch {
      throw Error(
        "This browser wouldn’t sign up for Orbit’s wake-ups. Check that it allows Orbit’s notifications, then try again.",
      );
    }
    const added = await action<AddedDevice>("add_push_device", {
      endpoint: signedUp.endpoint,
      name: deviceName(),
    });
    if (!added.ok) {
      // A sign-up made just now goes; one Orbit already wakes stays.
      if (made) await signedUp.unsubscribe().catch(() => false);
      throw Error(added.error);
    }
    try {
      await keep(added.data);
    } catch {
      // Without its key here, the worker couldn't read what it's woken
      // for. Orbit replaced any older one, so nothing is left to keep.
      await action("remove_push_device", { id: added.data.id });
      await forget();
      throw Error(
        "This browser wouldn’t keep the key it reads Orbit’s alerts with, so Orbit won’t wake it. Check that it has room for Orbit’s page, then try again.",
      );
    }
  });
}

/** Stops Orbit waking this browser. */
export function turnOffPush(action: Action): Promise<void> {
  return alone(async () => {
    const id = savedDevice();
    if (id) {
      const removed = await action("remove_push_device", { id });
      if (!removed.ok) throw Error(removed.error);
    }
    await forget();
  });
}

/** Stops Orbit waking the device `id`, from the list of them. When it's
 * this browser, it's signed out here too. */
export async function removeDevice(action: Action, id: string): Promise<void> {
  const remove = async () => {
    const removed = await action("remove_push_device", { id });
    if (!removed.ok) throw Error(removed.error);
    if (id === savedDevice()) await forget();
  };
  // Only this browser's own needs to wait for its other pages.
  await (id === savedDevice() ? alone(remove) : remove());
}

/** Forgets this browser's device here, once Orbit no longer wakes it. */
async function forget(): Promise<void> {
  kept = null;
  try {
    localStorage.removeItem(idKey);
  } catch {
    // Kept in memory only.
  }
  listeners.forEach((changed) => changed());
  try {
    const registration = await worker();
    const signedUp = await registration.pushManager.getSubscription();
    await signedUp?.unsubscribe();
  } catch {
    // Orbit no longer wakes it either way.
  }
  try {
    await caches.delete(cacheName);
  } catch {
    // Its key no longer reads anything.
  }
}

async function keep(device: AddedDevice) {
  const cache = await caches.open(cacheName);
  await cache.put(
    entry,
    new Response(JSON.stringify(device), {
      headers: { "Content-Type": "application/json" },
    }),
  );
  try {
    localStorage.setItem(idKey, device.id);
  } catch {
    kept = device.id;
  }
  listeners.forEach((changed) => changed());
}
