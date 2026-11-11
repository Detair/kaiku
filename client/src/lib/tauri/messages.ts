/**
 * Message commands: CRUD, threads, attachments, reactions, pins, channel pins,
 * unread aggregation, and signed URL fetching.
 */

import type {
  ChannelPin,
  CreatePinRequest,
  Message,
  PaginatedMessages,
  Pin,
  UpdatePinRequest,
} from "../types";
import {
  fetchApi,
  getUploadAuth,
  HttpError,
  refreshAccessToken,
  httpRequest,
  httpRequestWithStatus,
  validateFileSize,
} from "./common";

// ============================================================================
// Unread aggregation types
// ============================================================================

export interface ChannelUnread {
  channel_id: string;
  channel_name: string;
  unread_count: number;
  /** Read cursor (a top-level message id); null if never read. */
  last_read_message_id: string | null;
}

export interface GuildUnreadSummary {
  guild_id: string;
  guild_name: string;
  channels: ChannelUnread[];
  total_unread: number;
}

export interface UnreadAggregate {
  guilds: GuildUnreadSummary[];
  dms: ChannelUnread[];
  total: number;
}

// ============================================================================
// Messages
// ============================================================================

export async function getMessages(
  channelId: string,
  before?: string,
  limit?: number,
  around?: string,
): Promise<PaginatedMessages> {
  const params = new URLSearchParams();
  if (around) params.set("around", around);
  else if (before) params.set("before", before);
  if (limit) params.set("limit", limit.toString());
  const query = params.toString();

  return httpRequest<PaginatedMessages>(
    "GET",
    `/api/messages/channel/${channelId}${query ? `?${query}` : ""}`,
  );
}

/**
 * Catch-up page (C1): messages newer than `afterId`, oldest first.
 * `has_more` means newer messages remain; `next_cursor` is the newest id of
 * the fetched window. An unknown or foreign anchor is answered with 400.
 */
export async function getMessagesAfter(
  channelId: string,
  afterId: string,
  limit = 100,
): Promise<PaginatedMessages> {
  const params = new URLSearchParams({ after: afterId, limit: String(limit) });
  return httpRequest<PaginatedMessages>(
    "GET",
    `/api/messages/channel/${channelId}?${params.toString()}`,
  );
}

export async function sendMessage(
  channelId: string,
  content: string,
  options?: { encrypted?: boolean; nonce?: string },
): Promise<Message> {
  const result = await sendMessageWithStatus(channelId, content, options);
  return result.message;
}

export interface SendMessageResult {
  message: Message;
  status: number;
}

export async function sendMessageWithStatus(
  channelId: string,
  content: string,
  options?: { encrypted?: boolean; nonce?: string },
): Promise<SendMessageResult> {
  const { data, status } = await httpRequestWithStatus<Message>(
    "POST",
    `/api/messages/channel/${channelId}`,
    {
      content,
      encrypted: options?.encrypted ?? false,
      nonce: options?.nonce,
    },
  );
  return { message: data, status };
}

// ============================================================================
// Thread API Functions
// ============================================================================

export async function getThreadReplies(
  parentId: string,
  after?: string,
  limit?: number,
): Promise<PaginatedMessages> {
  const params = new URLSearchParams();
  if (after) params.set("after", after);
  if (limit) params.set("limit", limit.toString());
  const query = params.toString();

  return httpRequest<PaginatedMessages>(
    "GET",
    `/api/messages/${parentId}/thread${query ? `?${query}` : ""}`,
  );
}

export async function sendThreadReply(
  parentId: string,
  channelId: string,
  content: string,
  options?: { encrypted?: boolean; nonce?: string },
): Promise<Message> {
  return httpRequest<Message>("POST", `/api/messages/channel/${channelId}`, {
    content,
    encrypted: options?.encrypted ?? false,
    nonce: options?.nonce,
    parent_id: parentId,
  });
}

export async function markThreadRead(parentId: string): Promise<void> {
  return httpRequest<void>("POST", `/api/messages/${parentId}/thread/read`);
}

// ============================================================================
// File upload
// ============================================================================

export async function uploadFile(messageId: string, file: File): Promise<any> {
  // Frontend validation
  const error = validateFileSize(file, "attachment");
  if (error) {
    console.warn("[uploadFile] Frontend validation failed:", error);
    throw new Error(error);
  }

  const { token, baseUrl } = await getUploadAuth();

  const headers: Record<string, string> = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const formData = new FormData();
  formData.append("message_id", messageId);
  formData.append("file", file);

  const response = await fetch(`${baseUrl}/api/messages/upload`, {
    method: "POST",
    headers,
    body: formData,
  });

  if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;

    try {
      const errorBody = await response.json();
      errorMessage = errorBody.message || errorBody.error || errorMessage;
    } catch (parseError) {
      console.warn("[uploadFile] Failed to parse error response:", parseError);
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadFile] Upload failed:", {
      status: response.status,
      error: errorMessage,
      messageId,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error("[uploadFile] Failed to parse success response:", parseError);
    throw new Error("Server returned invalid response", { cause: parseError });
  }
}

/** Progress and cancellation for {@link uploadMessageWithFile}. */
export interface UploadOptions {
  /** Bytes sent so far and the total, as the browser reports them. */
  onProgress?: (loaded: number, total: number) => void;
  /** Aborting rejects with a `DOMException` named `AbortError`. */
  signal?: AbortSignal;
}

/** True for the rejection of an upload cancelled through its signal. */
export function isUploadAborted(err: unknown): boolean {
  return (
    typeof err === "object" &&
    err !== null &&
    (err as { name?: unknown }).name === "AbortError"
  );
}

function abortError(): DOMException {
  return new DOMException("Upload cancelled", "AbortError");
}

/** Error body `{ error, message }` (contract C16) as an `HttpError`. */
function uploadHttpError(status: number, statusText: string, rawBody: string): HttpError {
  let code: string | undefined;
  let message = statusText || `HTTP ${status}`;
  const details: Record<string, unknown> = {};
  try {
    const body = rawBody ? JSON.parse(rawBody) : null;
    if (body && typeof body === "object") {
      if (typeof body.error === "string") code = body.error;
      if (typeof body.message === "string" && body.message) message = body.message;
      else if (code) message = code;
      for (const [key, value] of Object.entries(body)) {
        if (key !== "error" && key !== "message") details[key] = value;
      }
    }
  } catch {
    // Not JSON (a proxy's 413 page, for example): status alone decides.
  }
  return new HttpError(status, message, code, details);
}

/**
 * Upload a file and create a message in one request.
 * Uses the combined endpoint that creates the message and attaches the file.
 *
 * Failures reject with an `HttpError` carrying the server code
 * (`INVALID_MIME_TYPE`, `FILE_TOO_LARGE`, …) so `errorText` can name the
 * reason; a lost connection rejects with a `TypeError`, a cancelled upload
 * with an `AbortError`.
 */
export async function uploadMessageWithFile(
  channelId: string,
  file: File,
  content?: string,
  options: UploadOptions = {},
): Promise<Message> {
  // Frontend validation
  const error = validateFileSize(file, "attachment");
  if (error) {
    console.warn("[uploadMessageWithFile] Frontend validation failed:", error);
    throw new HttpError(413, error, "FILE_TOO_LARGE");
  }
  if (options.signal?.aborted) throw abortError();

  const formData = new FormData();
  formData.append("file", file);
  if (content) {
    formData.append("content", content);
  }

  let response = await sendUpload(channelId, file, formData, options);
  // An expired session (laptop woke up): renew once and send again, as the
  // shared HTTP helper does.
  if (response.status === 401 && (await refreshAccessToken())) {
    response = await sendUpload(channelId, file, formData, options);
  }

  if (response.status >= 200 && response.status < 300) {
    try {
      return JSON.parse(response.responseText) as Message;
    } catch (parseError) {
      console.error("[uploadMessageWithFile] Failed to parse success response:", parseError);
      throw new Error("Server returned invalid response", { cause: parseError });
    }
  }
  const httpError = uploadHttpError(response.status, response.statusText, response.responseText);
  console.error("[uploadMessageWithFile] Upload failed:", {
    status: response.status,
    code: httpError.code,
    channelId,
    fileSize: file.size,
  });
  throw httpError;
}

interface UploadResponse {
  status: number;
  statusText: string;
  responseText: string;
}

/**
 * One upload request with the current token. Resolves with any HTTP answer;
 * rejects with a `TypeError` when the connection fails and an `AbortError`
 * when cancelled.
 */
async function sendUpload(
  channelId: string,
  file: File,
  formData: FormData,
  options: UploadOptions,
): Promise<UploadResponse> {
  if (options.signal?.aborted) throw abortError();
  const { token, baseUrl } = await getUploadAuth();
  if (options.signal?.aborted) throw abortError();

  const url = `${baseUrl}/api/messages/channel/${channelId}/upload`;

  return new Promise<UploadResponse>((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    const onAbort = () => xhr.abort();
    const done = () => options.signal?.removeEventListener("abort", onAbort);

    xhr.open("POST", url);
    if (token) xhr.setRequestHeader("Authorization", `Bearer ${token}`);

    if (options.onProgress && xhr.upload) {
      const report = options.onProgress;
      xhr.upload.onprogress = (e: ProgressEvent) => {
        report(e.loaded, e.lengthComputable ? e.total : file.size);
      };
    }

    xhr.onload = () => {
      done();
      resolve({ status: xhr.status, statusText: xhr.statusText, responseText: xhr.responseText });
    };
    // Same shape as a failed fetch, so errorText says "check your connection".
    xhr.onerror = () => {
      done();
      reject(new TypeError("Network request failed"));
    };
    xhr.onabort = () => {
      done();
      reject(abortError());
    };

    options.signal?.addEventListener("abort", onAbort, { once: true });
    xhr.send(formData);
  });
}

// ============================================================================
// Pins (user-level pin items, not message pinning)
// ============================================================================

export async function fetchPins(): Promise<Pin[]> {
  return httpRequest<Pin[]>("GET", "/api/me/pins");
}

export async function createPin(request: CreatePinRequest): Promise<Pin> {
  return httpRequest<Pin>("POST", "/api/me/pins", request);
}

export async function updatePin(
  pinId: string,
  request: UpdatePinRequest,
): Promise<Pin> {
  return httpRequest<Pin>("PUT", `/api/me/pins/${pinId}`, request);
}

export async function deletePin(pinId: string): Promise<void> {
  await httpRequest<void>("DELETE", `/api/me/pins/${pinId}`);
}

export async function reorderPins(pinIds: string[]): Promise<void> {
  await httpRequest<void>("PUT", "/api/me/pins/reorder", { pin_ids: pinIds });
}

// ============================================================================
// Channel Pins (message-level pinning)
// ============================================================================

/**
 * List pinned messages for a channel.
 */
export async function listChannelPins(channelId: string): Promise<ChannelPin[]> {
  return httpRequest<ChannelPin[]>("GET", `/api/channels/${channelId}/pins`);
}

/**
 * Pin a message to a channel.
 */
export async function pinMessage(channelId: string, messageId: string): Promise<void> {
  await httpRequest<void>("PUT", `/api/channels/${channelId}/messages/${messageId}/pin`);
}

/**
 * Unpin a message from a channel.
 */
export async function unpinMessage(channelId: string, messageId: string): Promise<void> {
  await httpRequest<void>("DELETE", `/api/channels/${channelId}/messages/${messageId}/pin`);
}

// ============================================================================
// Reactions
// ============================================================================

/**
 * Add a reaction to a message.
 */
export async function addReaction(
  channelId: string,
  messageId: string,
  emoji: string,
): Promise<void> {
  await httpRequest<void>(
    "PUT",
    `/api/channels/${channelId}/messages/${messageId}/reactions`,
    { emoji },
  );
}

/**
 * Remove a reaction from a message.
 */
export async function removeReaction(
  channelId: string,
  messageId: string,
  emoji: string,
): Promise<void> {
  await httpRequest<void>(
    "DELETE",
    `/api/channels/${channelId}/messages/${messageId}/reactions/${encodeURIComponent(emoji)}`,
  );
}

/**
 * Delete a message (soft delete, own messages only).
 */
export async function deleteMessage(messageId: string): Promise<void> {
  await httpRequest<void>("DELETE", `/api/messages/${messageId}`);
}

/**
 * Edit a message (own messages only).
 */
export async function editMessage(
  messageId: string,
  content: string,
): Promise<Message> {
  return httpRequest<Message>("PATCH", `/api/messages/${messageId}`, {
    content,
  });
}

// ============================================================================
// Unread aggregation / read-all
// ============================================================================

/**
 * Get aggregate unread counts across all guilds and DMs.
 * Returns unread counts grouped by guild, plus DM unreads.
 */
export async function getUnreadAggregate(): Promise<UnreadAggregate> {
  return fetchApi<UnreadAggregate>("/api/me/unread");
}

/**
 * Mark all text channels in a guild as read.
 */
export async function markAllGuildChannelsRead(guildId: string): Promise<void> {
  await fetchApi<void>(`/api/guilds/${guildId}/read-all`, { method: "POST" });
}

/**
 * Mark all DM channels as read.
 */
export async function markAllDMsRead(): Promise<void> {
  await fetchApi<void>("/api/dm/read-all", { method: "POST" });
}

/**
 * Mark everything (guilds + DMs) as read.
 */
export async function markAllRead(): Promise<void> {
  await fetchApi<void>("/api/me/read-all", { method: "POST" });
}

// ============================================================================
// Signed URLs (for attachments)
// ============================================================================

/**
 * Get a presigned S3 URL for downloading an attachment.
 * Uses Authorization header instead of passing JWT in the URL.
 */
export async function getSignedUrl(
  attachmentId: string,
  variant?: string,
): Promise<{ url: string; expires_in: number }> {
  const path = variant
    ? `/api/messages/attachments/${attachmentId}/url?variant=${encodeURIComponent(variant)}`
    : `/api/messages/attachments/${attachmentId}/url`;
  return httpRequest<{ url: string; expires_in: number }>("GET", path);
}
