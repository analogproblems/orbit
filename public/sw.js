// Orbit's service worker, for the page an Orbit serves to browsers (see
// src/notices.ts). The page shows each notification through it, which
// phones need, and a tap on one brings the page forward and opens its item.
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
