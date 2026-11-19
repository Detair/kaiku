/**
 * Channel rename and delete (ADMIN-5). The server has had
 * `PATCH/DELETE /api/channels/{id}` (MANAGE_CHANNELS) all along; the client
 * never called them.
 */
import type { Channel } from "../types";
import { httpRequest } from "./common";

export async function renameChannel(channelId: string, name: string): Promise<Channel> {
  return httpRequest<Channel>("PATCH", `/api/channels/${channelId}`, { name });
}

export async function deleteChannel(channelId: string): Promise<void> {
  await httpRequest<void>("DELETE", `/api/channels/${channelId}`);
}
