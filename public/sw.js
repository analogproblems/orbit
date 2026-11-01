// Orbit's service worker, for the page an Orbit serves to browsers (see
// src/notices.ts). The page shows each notification through it, which
// phones need, and a tap on one brings the page forward and opens its item.
// With alerts on while the page is closed (src/push.ts), it also shows each
// notification Orbit wakes it for.
self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) =>
  event.waitUntil(self.clients.claim()),
);
self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const item = event.notification.data && event.notification.data.item;
  event.waitUntil(
    (async () => {
      const pages = await self.clients.matchAll({
        type: "window",
        includeUncontrolled: true,
      });
      if (pages.length === 0) {
        // A page opened now shows Needs you, where the item is.
        await self.clients.openWindow("/");
        return;
      }
      const page = pages[0];
      await page.focus();
      if (item) page.postMessage({ type: "orbit-open", item });
    })(),
  );
});

// A wake-up from Orbit (push.rs), sent through this browser's push service
// when Orbit sends a notification. It carries nothing, so the push service
// never learns what's waiting: this reads that from Orbit itself, with this
// device's own key, which reads nothing else.
self.addEventListener("push", (event) => event.waitUntil(woken()));
async function woken() {
  const shown = (title, body, tag, data) =>
    self.registration.showNotification(title, { body, tag, data });
  let read;
  try {
    read = await whatsNew();
  } catch {
    read = { failed: true };
  }
  if (read.notices && read.notices.length > 0) {
    // The same tag as the page gives each, so it shows once.
    for (const n of read.notices) {
      await shown(n.title, n.body, n.id, { item: n.item });
    }
    return;
  }
  // Each wake-up must show something, or browsers stop them.
  if (read.forgotten) {
    await shown("Orbit", read.forgotten, "orbit-wake");
  } else if (read.failed) {
    await shown(
      "Something needs you",
      "Orbit couldn’t be reached to say what. Open Orbit to see it.",
      "orbit-wake",
    );
  } else {
    // Already read, by an earlier wake-up: show the latest again, quietly.
    const showing = await self.registration.getNotifications();
    const latest = showing[showing.length - 1];
    if (latest) {
      await self.registration.showNotification(latest.title, {
        body: latest.body,
        tag: latest.tag,
        data: latest.data,
        silent: true,
      });
    } else {
      await shown("Orbit", "Nothing new needs you now.", "orbit-wake");
    }
  }
}
/** What's new for this device: `{notices}`, `{forgotten: why}` when Orbit
 * no longer wakes it, or `{failed: true}` when Orbit couldn't say. */
async function whatsNew() {
  const cache = await caches.open("orbit-push");
  const saved = await cache.match("/orbit-push-device");
  const device = saved ? await saved.json() : null;
  if (!device || !device.key) return { failed: true };
  const response = await fetch("/push/notices", {
    method: "POST",
    headers: { Authorization: `Bearer ${device.key}` },
    cache: "no-store",
  });
  if (response.status === 401) {
    const said = await response.json().catch(() => ({}));
    await caches.delete("orbit-push");
    const signedUp = await self.registration.pushManager.getSubscription();
    if (signedUp) await signedUp.unsubscribe();
    return {
      forgotten:
        said.error ||
        "This device no longer gets Orbit’s alerts. Turn them on again in Settings, under Notifications.",
    };
  }
  if (!response.ok) return { failed: true };
  const body = await response.json();
  return { notices: Array.isArray(body.notices) ? body.notices : [] };
}
