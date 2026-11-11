/**
 * Attachment URLs for `<img src>`, links and downloads (C24).
 *
 * The access token never goes into a URL. The client asks the server for a
 * short-lived presigned URL (`GET /api/messages/attachments/{id}/url`, with
 * the Authorization header) and caches it until shortly before it expires,
 * so scrolling a message list does not ask again for every render. If the
 * presign request fails (e.g. storage without presigning), a file of at
 * most `BLOB_FALLBACK_MAX_BYTES` is fetched with the Authorization header
 * and shown from a blob URL; those blob URLs are owned by this cache and
 * revoked when evicted or cleared.
 *
 * The cache belongs to the signed-in session: sign-out and account switch
 * (`clearAttachmentUrls`) start a new generation, and a request of an older
 * generation that completes later is dropped (its blob revoked) instead of
 * landing in the next session's cache.
 */

import { getSignedUrl } from "@/lib/tauri/messages";
import { getAuthContext, HttpError } from "@/lib/tauri/common";

/** Ask again this long before the presigned URL expires. */
const REFRESH_MARGIN_MS = 60_000;
/** At most this many cached URLs; the least recently used go first. */
const MAX_ENTRIES = 200;
/**
 * Largest file the blob fallback loads into memory (25 MB). Inline images
 * and thumbnails are far below it; up to 200 cached entries can in the
 * worst case still pin a lot, but only in the rare no-presign setup. Above
 * it the message shows the file as a download instead of an inline image.
 */
export const BLOB_FALLBACK_MAX_BYTES = 25 * 1024 * 1024;

/** The attachment is gone or not ours (presign answered 403/404). */
export class AttachmentGoneError extends Error {
  constructor(public readonly status: number) {
    super(`Attachment unavailable: HTTP ${status}`);
    this.name = "AttachmentGoneError";
  }
}

/** The fallback would have to load a file above `BLOB_FALLBACK_MAX_BYTES`. */
export class AttachmentTooLargeError extends Error {
  constructor() {
    super("Attachment too large to load without a presigned URL");
    this.name = "AttachmentTooLargeError";
  }
}

/** A request of a session that has ended since (sign-out). */
export class AttachmentSessionEndedError extends Error {
  constructor() {
    super("Attachment request of an ended session");
    this.name = "AttachmentSessionEndedError";
  }
}

type Entry =
  | {
      kind: "url";
      url: string;
      /** Epoch ms after which the entry is asked for again. */
      refreshAt: number;
      /** A blob URL this cache created and must revoke. */
      blob: boolean;
    }
  | { kind: "gone"; status: number };

const cache = new Map<string, Entry>();
const inFlight = new Map<string, Promise<string>>();
/** Bumped by `clearAttachmentUrls` (sign-out, account switch). */
let generation = 0;

function keyOf(attachmentId: string, variant?: string): string {
  return `${attachmentId}:${variant ?? ""}`;
}

function evict(key: string): void {
  const entry = cache.get(key);
  if (entry?.kind === "url" && entry.blob) URL.revokeObjectURL(entry.url);
  cache.delete(key);
}

function remember(key: string, entry: Entry): void {
  evict(key);
  cache.set(key, entry);
  while (cache.size > MAX_ENTRIES) {
    // Map order is insertion order and hits re-insert: first = least recent.
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    evict(oldest);
  }
}

/** Mark `key` as just used (LRU). */
function touch(key: string, entry: Entry): void {
  cache.delete(key);
  cache.set(key, entry);
}

/** Fallback: fetch with the Authorization header and serve a blob URL. */
async function fetchAsBlob(attachmentId: string, variant?: string): Promise<Blob> {
  const { baseUrl, accessToken } = await getAuthContext();
  const query = variant ? `?variant=${encodeURIComponent(variant)}` : "";
  const headers: Record<string, string> = {};
  if (accessToken) headers.Authorization = `Bearer ${accessToken}`;
  const response = await fetch(
    `${baseUrl}/api/messages/attachments/${attachmentId}/download${query}`,
    { headers },
  );
  if (!response.ok) {
    if (response.status === 403 || response.status === 404) {
      throw new AttachmentGoneError(response.status);
    }
    throw new Error(`Attachment download failed: HTTP ${response.status}`);
  }
  const declared = Number(response.headers?.get?.("content-length") ?? NaN);
  if (declared > BLOB_FALLBACK_MAX_BYTES) {
    void response.body?.cancel().catch(() => {});
    throw new AttachmentTooLargeError();
  }
  const blob = await response.blob();
  if (blob.size > BLOB_FALLBACK_MAX_BYTES) throw new AttachmentTooLargeError();
  return blob;
}

function isGoneStatus(err: unknown): number | null {
  return err instanceof HttpError && (err.status === 403 || err.status === 404)
    ? err.status
    : null;
}

async function load(
  key: string,
  attachmentId: string,
  variant: string | undefined,
  opts: { sizeHint?: number },
): Promise<string> {
  const gen = generation;
  const current = () => gen === generation;
  try {
    const signed = await getSignedUrl(attachmentId, variant);
    if (!current()) throw new AttachmentSessionEndedError();
    const lifetimeMs = Math.max(0, signed.expires_in * 1000);
    // Refresh a minute early; for very short lifetimes, halfway.
    const margin = Math.min(REFRESH_MARGIN_MS, lifetimeMs / 2);
    remember(key, {
      kind: "url",
      url: signed.url,
      refreshAt: Date.now() + lifetimeMs - margin,
      blob: false,
    });
    return signed.url;
  } catch (err) {
    if (err instanceof AttachmentSessionEndedError || !current()) {
      throw new AttachmentSessionEndedError();
    }
    const gone = isGoneStatus(err);
    if (gone !== null) {
      // Deleted or not visible: remembered for this session, not retried.
      remember(key, { kind: "gone", status: gone });
      throw new AttachmentGoneError(gone);
    }
    if ((opts.sizeHint ?? 0) > BLOB_FALLBACK_MAX_BYTES) throw new AttachmentTooLargeError();
    console.warn("[Attachments] Presigned URL failed, loading the file directly:", err);
    let blob: Blob;
    try {
      blob = await fetchAsBlob(attachmentId, variant);
    } catch (fallbackErr) {
      if (current() && fallbackErr instanceof AttachmentGoneError) {
        remember(key, { kind: "gone", status: fallbackErr.status });
      }
      throw fallbackErr;
    }
    // Signed out meanwhile: nothing of the old session is kept.
    if (!current()) throw new AttachmentSessionEndedError();
    const url = URL.createObjectURL(blob);
    remember(key, { kind: "url", url, refreshAt: Number.POSITIVE_INFINITY, blob: true });
    return url;
  }
}

/**
 * A URL for the attachment (optionally a variant such as "thumbnail") that
 * needs no token in it. Cached per session; concurrent callers share one
 * request. `sizeHint` (bytes) skips a blob fallback that would be too big.
 */
export function getAttachmentUrl(
  attachmentId: string,
  variant?: string,
  opts: { sizeHint?: number } = {},
): Promise<string> {
  const key = keyOf(attachmentId, variant);
  const cached = cache.get(key);
  if (cached?.kind === "gone") return Promise.reject(new AttachmentGoneError(cached.status));
  if (cached && Date.now() < cached.refreshAt) {
    touch(key, cached);
    return Promise.resolve(cached.url);
  }
  const pending = inFlight.get(key);
  if (pending) return pending;
  const gen = generation;
  const request = load(key, attachmentId, variant, opts).finally(() => {
    if (gen === generation) inFlight.delete(key);
  });
  inFlight.set(key, request);
  return request;
}

/**
 * The URL handed out for `attachmentId` did not load (an `<img>` error,
 * e.g. it expired while the image waited off-screen): forget it so the
 * next `getAttachmentUrl` asks again. "Gone" entries stay.
 */
export function invalidateAttachmentUrl(attachmentId: string, variant?: string): void {
  const key = keyOf(attachmentId, variant);
  if (cache.get(key)?.kind === "url") evict(key);
}

/** Whether an attachment error means "show it as a download, not inline". */
export function isAttachmentTooLarge(err: unknown): boolean {
  return err instanceof AttachmentTooLargeError;
}

/** Drop every cached URL, revoke the blob URLs, end the session (sign-out). */
export function clearAttachmentUrls(): void {
  generation += 1;
  for (const key of [...cache.keys()]) evict(key);
  inFlight.clear();
}
