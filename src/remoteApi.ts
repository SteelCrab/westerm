// Calls to the dashboard's remote westerm APIs (see shared/protocol.ts).

/** POSTs JSON to a dashboard API. True on a 2xx answer, false on anything else. */
export async function postRemote(path: string, body: Record<string, unknown>): Promise<boolean> {
  try {
    const response = await fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return response.ok;
  } catch {
    return false;
  }
}
