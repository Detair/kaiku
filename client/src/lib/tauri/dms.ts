/**
 * Direct message commands: CRUD, avatar upload, name updates, and DM calls.
 */

import type {
  CallStateResponse,
  DMChannel,
  DMListItem,
} from "../types";
import {
  getAccessToken,
  getServerUrl,
  httpRequest,
  isTauri,
  validateFileSize,
} from "./common";

// ============================================================================
// DM Avatar upload
// ============================================================================

export interface DMIconResponse {
  icon_url: string;
}

export async function uploadDMAvatar(
  channelId: string,
  file: File,
): Promise<DMIconResponse> {
  // Frontend validation
  const validationError = validateFileSize(file, "avatar");
  if (validationError) {
    console.warn(
      "[uploadDMAvatar] Frontend validation failed:",
      validationError,
    );
    throw new Error(validationError);
  }

  const formData = new FormData();
  formData.append("file", file);

  const token = getAccessToken();
  const headers: HeadersInit = {};
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const response = await fetch(`${getServerUrl()}/api/dm/${channelId}/icon`, {
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
      console.warn(
        "[uploadDMAvatar] Failed to parse error response:",
        parseError,
      );
      errorMessage = response.statusText || errorMessage;
    }

    console.error("[uploadDMAvatar] Upload failed:", {
      status: response.status,
      error: errorMessage,
      channelId,
      fileSize: file.size,
      fileName: file.name,
    });

    throw new Error(errorMessage);
  }

  try {
    return await response.json();
  } catch (parseError) {
    console.error(
      "[uploadDMAvatar] Failed to parse success response:",
      parseError,
    );
    throw new Error("Server returned invalid response", { cause: parseError });
  }
}

// ============================================================================
// DM CRUD
// ============================================================================

export async function getDMs(): Promise<DMChannel[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_dms");
  }

  return httpRequest<DMChannel[]>("GET", "/api/dm");
}

export async function getDM(channelId: string): Promise<DMChannel> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_dm", { channelId });
  }

  return httpRequest<DMChannel>("GET", `/api/dm/${channelId}`);
}

export async function createDM(
  participantIds: string[],
  name?: string,
): Promise<DMChannel> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_dm", { participantIds, name });
  }

  return httpRequest<DMChannel>("POST", "/api/dm", {
    participant_ids: participantIds,
    name,
  });
}

export async function leaveDM(channelId: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_dm", { channelId });
  }

  await httpRequest<void>("POST", `/api/dm/${channelId}/leave`);
}

export async function getDMList(): Promise<DMListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_dm_list");
  }

  return httpRequest<DMListItem[]>("GET", "/api/dm");
}

export async function markDMAsRead(
  channelId: string,
  lastReadMessageId: string,
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mark_dm_as_read", { channelId, lastReadMessageId });
  }

  await httpRequest<void>("POST", `/api/dm/${channelId}/read`, {
    last_read_message_id: lastReadMessageId,
  });
}

/**
 * Update the display name of a group DM channel.
 */
export async function updateDMName(
  channelId: string,
  name: string,
): Promise<void> {
  await httpRequest<void>("PATCH", `/api/dm/${channelId}/name`, { name });
}

// ============================================================================
// DM Call Commands
// ============================================================================

/**
 * Get the current call state for a DM channel.
 */
export async function getCallState(
  channelId: string,
): Promise<CallStateResponse | null> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_call_state", { channelId });
  }

  return httpRequest<CallStateResponse | null>(
    "GET",
    `/api/dm/${channelId}/call`,
  );
}

/**
 * Start a new call in a DM channel.
 */
export async function startDMCall(
  channelId: string,
): Promise<CallStateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("start_dm_call", { channelId });
  }

  return httpRequest<CallStateResponse>(
    "POST",
    `/api/dm/${channelId}/call/start`,
  );
}

/**
 * Join an active call in a DM channel.
 */
export async function joinDMCall(
  channelId: string,
): Promise<CallStateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("join_dm_call", { channelId });
  }

  return httpRequest<CallStateResponse>(
    "POST",
    `/api/dm/${channelId}/call/join`,
  );
}

/**
 * Decline an incoming call in a DM channel.
 */
export async function declineDMCall(
  channelId: string,
): Promise<CallStateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("decline_dm_call", { channelId });
  }

  return httpRequest<CallStateResponse>(
    "POST",
    `/api/dm/${channelId}/call/decline`,
  );
}

/**
 * Leave an active call in a DM channel.
 */
export async function leaveDMCall(
  channelId: string,
): Promise<CallStateResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_dm_call", { channelId });
  }

  return httpRequest<CallStateResponse>(
    "POST",
    `/api/dm/${channelId}/call/leave`,
  );
}
