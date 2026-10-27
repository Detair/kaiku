/**
 * How many messages were appended after `prevLastId`, the newest message at
 * the previous check. Older pages are inserted in front of it and do not
 * count. Without an anchor in the list (first load, or a pending message that
 * was replaced by its confirmed copy) the growth in count is used.
 */
export function countAppended(
  messages: readonly { id: string }[],
  prevLastId: string | undefined,
  prevCount: number,
): number {
  const prevLastIndex = prevLastId
    ? messages.findIndex((m) => m.id === prevLastId)
    : -1;
  return prevLastIndex === -1
    ? messages.length - prevCount
    : messages.length - 1 - prevLastIndex;
}
