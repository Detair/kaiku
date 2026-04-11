/**
 * Social commands: friends and user blocking.
 */

import type { Friend, Friendship } from "../types";
import { httpRequest } from "./common";

// ============================================================================
// Friends
// ============================================================================

export async function getFriends(): Promise<Friend[]> {
  return httpRequest<Friend[]>("GET", "/api/friends");
}

export async function getPendingFriends(): Promise<Friend[]> {
  return httpRequest<Friend[]>("GET", "/api/friends/pending");
}

export async function getBlockedFriends(): Promise<Friend[]> {
  return httpRequest<Friend[]>("GET", "/api/friends/blocked");
}

export async function sendFriendRequest(username: string): Promise<Friendship> {
  return httpRequest<Friendship>("POST", "/api/friends/request", { username });
}

export async function acceptFriendRequest(
  friendshipId: string,
): Promise<Friendship> {
  return httpRequest<Friendship>("POST", `/api/friends/${friendshipId}/accept`);
}

export async function rejectFriendRequest(friendshipId: string): Promise<void> {
  await httpRequest<void>("POST", `/api/friends/${friendshipId}/reject`);
}

export async function removeFriend(friendshipId: string): Promise<void> {
  await httpRequest<void>("DELETE", `/api/friends/${friendshipId}`);
}

// ============================================================================
// Blocking
// ============================================================================

export async function blockUser(userId: string): Promise<Friendship> {
  return httpRequest<Friendship>("POST", `/api/friends/${userId}/block`);
}

export async function unblockUser(userId: string): Promise<void> {
  await httpRequest<void>("DELETE", `/api/friends/${userId}/block`);
}

// ============================================================================
// Reports (user-created, viewed by admins via admin.ts)
// ============================================================================

export interface CreateReportRequest {
  target_type: "user" | "message";
  target_user_id: string;
  target_message_id?: string;
  category:
  | "harassment"
  | "spam"
  | "inappropriate_content"
  | "impersonation"
  | "other";
  description?: string;
}

export interface ReportResponse {
  id: string;
  reporter_id: string;
  target_type: string;
  target_user_id: string;
  target_message_id: string | null;
  category: string;
  description: string | null;
  status: string;
  created_at: string;
}

export async function createReport(
  request: CreateReportRequest,
): Promise<ReportResponse> {
  return httpRequest<ReportResponse>("POST", "/api/reports", request);
}
