import { apiGet, apiSend } from '$lib/api';

/**
 * Typed client for the notification inbox (`gw_api::routes::notifications`, ADR 0022/0024).
 *
 * Everything here is built from the fields the API returns: an actor's display name and the
 * page's title and path. Never page text. The API already filtered the list through the
 * reader's current permissions at delivery; nothing here filters again.
 */
export type NotificationKind =
  | 'comment_reply'
  | 'mention'
  | 'page_edited'
  | 'task_assigned'
  | 'task_due'
  | 'invite_accepted'
  | 'grant_changed';

export interface Notification {
  id: string;
  kind: NotificationKind;
  created_at: string;
  read: boolean;
  actor_name: string | null;
  page: { path: string; title: string | null };
}

export const NOTIFICATIONS_PATH = '/benachrichtigungen';

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

export interface Described {
  /** Who did it; `null` when the API names nobody or the kind has no actor (a due date). */
  actor: string | null;
  /** The sentence between the actor and the page. */
  text: string;
  /** The page's title, else its path. */
  page: string;
}

const TEXT: Record<NotificationKind, string> = {
  comment_reply: 'hat auf Ihren Kommentar geantwortet auf',
  mention: 'hat Sie erwähnt auf',
  page_edited: 'hat die Seite bearbeitet:',
  task_assigned: 'hat Ihnen eine Aufgabe zugewiesen auf',
  task_due: 'Eine Ihrer Aufgaben ist fällig auf',
  invite_accepted: 'hat eine Einladung angenommen für',
  grant_changed: 'hat eine Freigabe geändert für'
};

/** German sentence parts for one notification, from API fields only. */
export function describe(n: Notification): Described {
  const page = n.page.title?.trim() || n.page.path;
  const base = TEXT[n.kind];
  if (!base) return { actor: null, text: 'Neuigkeit auf', page };
  if (n.kind === 'task_due') return { actor: null, text: base, page };
  if (n.actor_name) return { actor: n.actor_name, text: base, page };
  return { actor: null, text: base.replace(/^hat /, 'Jemand hat '), page };
}

/** Message for a failed list request; 401 means signed out. */
export function describeFailure(status: number): string {
  if (status === 401) return 'Bitte melden Sie sich an, um Benachrichtigungen zu sehen.';
  if (status === 0) return 'Die Benachrichtigungen konnten nicht geladen werden: keine Antwort.';
  return `Die Benachrichtigungen konnten nicht geladen werden (Fehler ${status}).`;
}
