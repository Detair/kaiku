/**
 * WebSocket ClientEvent and ServerEvent union types.
 */

import type { UserStatus } from "./common";
import type { Activity, CustomStatus } from "./user";
import type { Message, ThreadInfo } from "./message";
import type { GuildEmoji } from "./guild";
import type {
  VoiceParticipant,
  ScreenShareServerInfo,
} from "./voice";
import type { UserPreferences } from "./preferences";

// WebSocket Events

export type ClientEvent =
  | { type: "ping" }
  | { type: "subscribe"; channel_id: string }
  | { type: "unsubscribe"; channel_id: string }
  | { type: "typing"; channel_id: string }
  | { type: "stop_typing"; channel_id: string }
  | { type: "voice_join"; channel_id: string }
  | { type: "voice_leave"; channel_id: string }
  | { type: "voice_publisher_offer"; channel_id: string; sdp: string }
  | { type: "voice_subscriber_answer"; channel_id: string; sdp: string }
  | { type: "voice_ice_candidate"; channel_id: string; candidate: string; pc_type?: string }
  | { type: "voice_mute"; channel_id: string }
  | { type: "voice_unmute"; channel_id: string }
  // Webcam events
  | { type: "voice_webcam_start"; channel_id: string; quality: string }
  | { type: "voice_webcam_stop"; channel_id: string }
  // Simulcast layer preference
  | {
      type: "voice_set_layer_preference";
      channel_id: string;
      target_user_id: string;
      track_source: string;
      preferred_layer: "auto" | "high" | "medium" | "low";
    }
  // Custom status events
  | { type: "set_custom_status"; custom_status: CustomStatus | null }
  // Admin events
  | { type: "admin_subscribe" }
  | { type: "admin_unsubscribe" };

export type ServerEvent =
  | { type: "ready"; user_id: string }
  | { type: "pong" }
  | { type: "subscribed"; channel_id: string }
  | { type: "unsubscribed"; channel_id: string }
  | { type: "message_new"; channel_id: string; message: Message }
  | {
      type: "message_edit";
      channel_id: string;
      message_id: string;
      content: string;
      edited_at: string;
    }
  | { type: "message_delete"; channel_id: string; message_id: string }
  | { type: "typing_start"; channel_id: string; user_id: string }
  | { type: "typing_stop"; channel_id: string; user_id: string }
  | { type: "presence_update"; user_id: string; status: UserStatus }
  | { type: "rich_presence_update"; user_id: string; activity: Activity | null }
  | { type: "custom_status_update"; user_id: string; custom_status: CustomStatus | null }
  | { type: "voice_publisher_answer"; channel_id: string; sdp: string }
  | { type: "voice_subscriber_offer"; channel_id: string; sdp: string }
  | { type: "voice_ice_candidate"; channel_id: string; candidate: string; pc_type?: string }
  | {
      type: "voice_user_joined";
      channel_id: string;
      user_id: string;
      username: string;
      display_name: string;
    }
  | { type: "voice_user_left"; channel_id: string; user_id: string }
  | { type: "voice_user_muted"; channel_id: string; user_id: string }
  | { type: "voice_user_unmuted"; channel_id: string; user_id: string }
  | {
      type: "voice_room_state";
      channel_id: string;
      participants: VoiceParticipant[];
      screen_shares?: ScreenShareServerInfo[];
      webcams?: import("./voice").WebcamServerInfo[];
    }
  | { type: "voice_error"; code: string; message: string }
  // Screen share events
  | {
      type: "screen_share_started";
      channel_id: string;
      user_id: string;
      stream_id: string;
      username: string;
      source_label: string;
      has_audio: boolean;
      quality: "low" | "medium" | "high" | "premium";
      started_at?: string;
    }
  | {
      type: "screen_share_stopped";
      channel_id: string;
      user_id: string;
      stream_id: string;
      reason: string;
    }
  | {
      type: "screen_share_quality_changed";
      channel_id: string;
      user_id: string;
      stream_id: string;
      new_quality: "low" | "medium" | "high" | "premium";
    }
  // Webcam events
  | {
      type: "webcam_started";
      channel_id: string;
      user_id: string;
      username: string;
      quality: "low" | "medium" | "high" | "premium";
    }
  | {
      type: "webcam_stopped";
      channel_id: string;
      user_id: string;
      reason: string;
    }
  | { type: "error"; code: string; message: string }
  // Call events
  | {
      type: "incoming_call";
      channel_id: string;
      initiator: string;
      initiator_name: string;
    }
  | { type: "call_started"; channel_id: string }
  | {
      type: "call_ended";
      channel_id: string;
      reason: string;
      duration_secs: number | null;
    }
  | {
      type: "call_participant_joined";
      channel_id: string;
      user_id: string;
      username: string;
    }
  | { type: "call_participant_left"; channel_id: string; user_id: string }
  | { type: "call_declined"; channel_id: string; user_id: string }
  // Voice metrics events
  | {
      type: "voice_user_stats";
      channel_id: string;
      user_id: string;
      latency: number;
      packet_loss: number;
      jitter: number;
      quality: number;
    }
  // Simulcast layer events
  | {
      type: "voice_layer_changed";
      channel_id: string;
      source_user_id: string;
      track_source: string;
      active_layer: "high" | "medium" | "low";
    }
  // Admin events
  | { type: "admin_user_banned"; user_id: string; username: string }
  | { type: "admin_user_unbanned"; user_id: string; username: string }
  | { type: "admin_guild_suspended"; guild_id: string; guild_name: string }
  | { type: "admin_guild_unsuspended"; guild_id: string; guild_name: string }
  | { type: "admin_user_deleted"; user_id: string; username: string }
  | { type: "admin_guild_deleted"; guild_id: string; guild_name: string }
  // DM read sync event
  | { type: "dm_read"; channel_id: string; last_read_message_id?: string }
  // Guild channel read sync event
  | { type: "channel_read"; channel_id: string; last_read_message_id?: string }
  // Preferences events
  | {
      type: "preferences_updated";
      preferences: Partial<UserPreferences>;
      updated_at: string;
    }
  // Reaction events
  | {
      type: "reaction_add";
      channel_id: string;
      message_id: string;
      user_id: string;
      emoji: string;
    }
  | {
      type: "reaction_remove";
      channel_id: string;
      message_id: string;
      user_id: string;
      emoji: string;
    }
  // Channel pin events
  | {
      type: "channel_pin_added";
      channel_id: string;
      message_id: string;
      pinned_by: string;
      pinned_at: string;
    }
  | { type: "channel_pin_removed"; channel_id: string; message_id: string }
  // Guild emoji events
  | { type: "guild_emoji_updated"; guild_id: string; emojis: GuildEmoji[] }
  // Member role changes (reaction-roles + admin assign/remove)
  | {
      type: "member_roles_updated";
      guild_id: string;
      user_id: string;
      role_ids: string[];
    }
  // Friend events
  | {
      type: "friend_request_received";
      friendship_id: string;
      from_user_id: string;
      from_username: string;
      from_display_name: string;
      from_avatar_url: string | null;
    }
  | {
      type: "friend_request_accepted";
      friendship_id: string;
      user_id: string;
      username: string;
      display_name: string;
      avatar_url: string | null;
    }
  | {
      type: "friend_request_rejected";
      friendship_id: string;
    }
  // DM metadata events
  | {
      type: "dm_name_updated";
      channel_id: string;
      name: string;
      updated_by: string;
    }
  // Block events
  | { type: "user_blocked"; user_id: string }
  | { type: "user_unblocked"; user_id: string }
  // Admin report events
  | {
      type: "admin_report_created";
      report_id: string;
      category: string;
      target_type: string;
    }
  | { type: "admin_report_resolved"; report_id: string }
  // Thread events
  | {
      type: "thread_reply_new";
      channel_id: string;
      parent_id: string;
      message: Message;
      thread_info: ThreadInfo;
    }
  | {
      type: "thread_reply_delete";
      channel_id: string;
      parent_id: string;
      message_id: string;
      thread_info: ThreadInfo;
    }
  | {
      type: "thread_read";
      thread_parent_id: string;
      last_read_message_id: string | null;
    }
  // State sync events
  | {
      type: "patch";
      entity_type: string;
      entity_id: string;
      diff: Record<string, unknown>;
    }
  // Bot command response events
  | {
      type: "command_response";
      interaction_id: string;
      content: string;
      command_name: string;
      bot_name: string;
      channel_id: string;
      ephemeral: boolean;
    }
  | {
      type: "command_response_timeout";
      interaction_id: string;
      command_name: string;
      channel_id: string;
    };
