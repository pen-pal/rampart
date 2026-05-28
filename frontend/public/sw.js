// Rampart service worker — handles incoming Web Push and notification clicks.
// Registered at the site root so its scope covers the whole app.

self.addEventListener('push', (event) => {
  let data = {};
  try {
    data = event.data ? event.data.json() : {};
  } catch (_e) {
    data = { title: 'Rampart', body: event.data ? event.data.text() : '' };
  }
  const title = data.title || 'Rampart alert';
  const body  = data.body  || '';
  const status = (data.status || '').toLowerCase();
  // Tag by monitor so repeated alerts for the same monitor collapse.
  const tag = data.monitor_id ? `rampart-${data.monitor_id}` : 'rampart';

  event.waitUntil(
    self.registration.showNotification(title, {
      body,
      tag,
      renotify: true,
      timestamp: Date.now(),
      // Down alerts require interaction; recoveries auto-dismiss.
      requireInteraction: status === 'down',
      data: { monitor_id: data.monitor_id || null },
    })
  );
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const mid = event.notification.data && event.notification.data.monitor_id;
  const url = mid ? `/#/monitor/${mid}` : '/#/';
  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((clients) => {
      // Focus an existing tab if one is open; otherwise open a new one.
      for (const c of clients) {
        if ('focus' in c) { c.navigate(url); return c.focus(); }
      }
      if (self.clients.openWindow) return self.clients.openWindow(url);
    })
  );
});
