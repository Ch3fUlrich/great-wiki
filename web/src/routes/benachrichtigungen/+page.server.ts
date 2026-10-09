import { redirect } from '@sveltejs/kit';
import { describeFailure, NOTIFICATIONS_PATH, type Notification } from '$lib/notifications';
import { listNotifications, markAllRead } from '$lib/notificationsApi';
import type { Actions, PageServerLoad } from './$types';

/**
 * The inbox. A failed request is stated, never rendered as an empty inbox; signed-out (401)
 * gets its own wording, the way the board words a status.
 */
export const load: PageServerLoad = async ({ fetch, request }) => {
  const cookie = request.headers.get('cookie');
  let notifications: Notification[] = [];
  let fehler: string | null = null;
  try {
    const answer = await listNotifications(fetch, cookie);
    if (answer.data) notifications = answer.data.notifications ?? [];
    else fehler = describeFailure(answer.status);
  } catch {
    fehler = describeFailure(0);
  }
  return { notifications, fehler };
};

export const actions: Actions = {
  alleGelesen: async ({ fetch, request }) => {
    await markAllRead(fetch, request.headers.get('cookie'));
    redirect(303, NOTIFICATIONS_PATH);
  }
};
