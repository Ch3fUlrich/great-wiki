/** A same-origin write from the browser: status plus the API's own `{"error": …}` words. */
export async function apiSendBrowser(
  method: 'POST' | 'PATCH' | 'DELETE',
  path: string,
  body: unknown
): Promise<{ status: number; message: string | null }> {
  try {
    const res = await fetch(path, {
      method,
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body)
    });
    let message: string | null = null;
    if (!res.ok) {
      try {
        const j = (await res.json()) as { error?: unknown };
        if (typeof j.error === 'string') message = j.error;
      } catch {
        /* a refusal without a body is just a status */
      }
    }
    return { status: res.status, message };
  } catch {
    return { status: 0, message: null };
  }
}
