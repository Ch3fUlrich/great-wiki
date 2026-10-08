import { apiGet, apiSend } from '$lib/api';
import type { Notification } from '$lib/notifications';

// Server-only (`$lib/api` reads `$env/dynamic/private`): kept apart from `notifications.ts`
// so the page component can import `describe` without pulling the server module into the
// browser bundle.

type Fetch = typeof fetch;

/** `GET /api/notifications?limit=` */
export function listNotifications(fetchFn: Fetch, cookie: string | null, limit = 50) {
  return apiGet<{ notifications: Notification[] }>(
    fetchFn,
    `/api/notifications?limit=${encodeURIComponent(String(limit))}`,
    cookie
  );
}

/** `GET /api/notifications/unread-count` */
export function unreadCount(fetchFn: Fetch, cookie: string | null) {
  return apiGet<{ count: number }>(fetchFn, '/api/notifications/unread-count', cookie);
}

/** `POST /api/notifications/{id}/read` */
export function markRead(fetchFn: Fetch, cookie: string | null, id: string) {
  return apiSend<unknown>(
    fetchFn,
    'POST',
    `/api/notifications/${encodeURIComponent(id)}/read`,
    cookie
  );
}

/** `POST /api/notifications/read-all` */
export function markAllRead(fetchFn: Fetch, cookie: string | null) {
  return apiSend<unknown>(fetchFn, 'POST', '/api/notifications/read-all', cookie);
}
