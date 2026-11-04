/**
 * Sessions the user is closing. A pane unmounts both when it is closed and
 * when it is only remounted (restart, hot reload); only a closed one should
 * stop its process. The dashboard marks ids here right before removing them.
 */
const ending = new Set<string>();

export function markEnding(ids: Iterable<string>): void {
  for (const id of ids) ending.add(id);
}

/** True once for a marked id. */
export function takeEnding(id: string): boolean {
  return ending.delete(id);
}
