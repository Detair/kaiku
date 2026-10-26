/**
 * Newest message the server accepts as a read cursor: top-level (thread
 * replies are rejected) and confirmed (not an optimistic "pending:" id).
 */
export function lastMarkableMessage<T extends { id: string; parent_id: string | null }>(
  msgs: readonly T[],
): T | undefined {
  for (let i = msgs.length - 1; i >= 0; i--) {
    if (msgs[i].parent_id == null && !msgs[i].id.startsWith("pending:")) {
      return msgs[i];
    }
  }
  return undefined;
}
