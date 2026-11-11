/**
 * Attachment URLs for `<img src>`, links and downloads (C24).
 *
 * The access token never goes into a URL. The client asks the server for a
 * short-lived presigned URL (`GET /api/messages/attachments/{id}/url`, with
 * the Authorization header) and caches it until shortly before it expires,
 * so scrolling a message list does not ask again for every render. If the
 * presign request fails (e.g. storage without presigning), the file is
 * fetched with the Authorization header and shown from a blob URL; those
 * blob URLs are owned by this cache and revoked when evicted or cleared.
 */

import { getSignedUrl } from "@/lib/tauri/messages";
import { getAuthContext } from "@/lib/tauri/common";

/** Ask again this long before the presigned URL expires. */
const REFRESH_MARGIN_MS = 60_000;
/** At most this many cached URLs; the oldest (and its blob URL) go first. */
const MAX_ENTRIES = 200;

interface Entry {
  url: string;
  /** Epoch ms after which the entry is asked for again. */
  refreshAt: number;
  /** A blob URL this cache created and must revoke. */
  blob: boolean;
}

const cache = new Map<string, Entry>();
const inFlight = new Map<string, Promise<string>>();

function keyOf(attachmentId: string, variant?: string): string {
  return `${attachmentId}:${variant ?? ""}`;
}

function evict(key: string): void {
  const entry = cache.get(key);
  if (entry?.blob) URL.revokeObjectURL(entry.url);
  cache.delete(key);
}

function remember(key: string, entry: Entry): void {
  evict(key);
  cache.set(key, entry);
  while (cache.size > MAX_ENTRIES) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    evict(oldest);
  }
}

/** Fallback: fetch with the Authorization header and serve a blob URL. */
async function fetchAsBlobUrl(attachmentId: string, variant?: string): Promise<string> {
  const { baseUrl, accessToken } = await getAuthContext();
  const query = variant ? `?variant=${encodeURIComponent(variant)}` : "";
  const headers: Record<string, string> = {};
  if (accessToken) headers.Authorization = `Bearer ${accessToken}`;
  const response = await fetch(
    `${baseUrl}/api/messages/attachments/${attachmentId}/download${query}`,
    { headers },
  );
  if (!response.ok) {
    throw new Error(`Attachment download failed: HTTP ${response.status}`);
  }
  return URL.createObjectURL(await response.blob());
}

async function load(key: string, attachmentId: string, variant?: string): Promise<string> {
  try {
    const signed = await getSignedUrl(attachmentId, variant);
    const lifetimeMs = Math.max(0, signed.expires_in * 1000);
    // Refresh a minute early; for very short lifetimes, halfway.
    const margin = Math.min(REFRESH_MARGIN_MS, lifetimeMs / 2);
    remember(key, { url: signed.url, refreshAt: Date.now() + lifetimeMs - margin, blob: false });
    return signed.url;
  } catch (err) {
    console.warn("[Attachments] Presigned URL failed, loading the file directly:", err);
    const url = await fetchAsBlobUrl(attachmentId, variant);
    remember(key, { url, refreshAt: Number.POSITIVE_INFINITY, blob: true });
    return url;
  }
}

/**
 * A URL for the attachment (optionally a variant such as "thumbnail") that
 * needs no token in it. Cached; concurrent callers share one request.
 */
export function getAttachmentUrl(attachmentId: string, variant?: string): Promise<string> {
  const key = keyOf(attachmentId, variant);
  const cached = cache.get(key);
  if (cached && Date.now() < cached.refreshAt) return Promise.resolve(cached.url);
  const pending = inFlight.get(key);
  if (pending) return pending;
  const request = load(key, attachmentId, variant).finally(() => inFlight.delete(key));
  inFlight.set(key, request);
  return request;
}

/** Drop every cached URL and revoke the blob URLs (sign-out, tests). */
export function clearAttachmentUrls(): void {
  for (const key of [...cache.keys()]) evict(key);
  inFlight.clear();
}
