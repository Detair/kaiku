/**
 * The Kaiku DM (C28): recognised only by `read_only` on the DM and a
 * participant with `is_system` — never by id or name, so a user who calls
 * themselves "kaiku" is an ordinary DM.
 */

interface DmLike {
  read_only?: boolean;
  participants: { user_id: string; is_system?: boolean }[];
}

export function isKaikuDM(dm: DmLike | null | undefined): boolean {
  return !!dm && dm.read_only === true && dm.participants.some((p) => p.is_system === true);
}

/**
 * A DM nobody can write to (C28 `read_only`, the flag alone): no composer,
 * no calls, no message actions (Fable review m3). `isKaikuDM` only decides
 * the Kaiku name and badge.
 */
export function isReadOnlyDM(dm: Pick<DmLike, "read_only"> | null | undefined): boolean {
  return dm?.read_only === true;
}
