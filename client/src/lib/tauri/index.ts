/**
 * Tauri Command Wrappers
 *
 * Type-safe wrappers for Tauri commands. Falls back to HTTP API when running
 * in a browser. This barrel re-exports every domain module so consumers can
 * continue importing from `@/lib/tauri` after the split.
 */

// Re-export types from the central types module for convenience, matching
// the legacy tauri.ts surface so consumers don't need to touch their imports.
export type {
  User,
  Channel,
  ChannelCategory,
  ChannelWithUnread,
  Message,
  AppSettings,
  Guild,
  GuildMember,
  GuildInvite,
  InviteResponse,
  InviteExpiry,
  Friend,
  Friendship,
  DMChannel,
  DMListItem,
  Page,
  PageListItem,
  GuildRole,
  GuildEmoji,
  ChannelOverride,
  CreateRoleRequest,
  UpdateRoleRequest,
  SetChannelOverrideRequest,
  AssignRoleResponse,
  RemoveRoleResponse,
  DeleteRoleResponse,
  AdminStats,
  AdminStatus,
  UserSummary,
  GuildSummary,
  AuditLogEntry,
  PaginatedResponse,
  ElevateResponse,
  UserDetailsResponse,
  GuildDetailsResponse,
  BulkBanResponse,
  BulkSuspendResponse,
  CallEndReason,
  CallStateResponse,
  E2EEStatus,
  InitE2EEResponse,
  PrekeyData,
  E2EEContent,
  ClaimedPrekeyInput,
  UserKeysResponse,
  ClaimedPrekeyResponse,
  SearchResponse,
  SearchFilters,
  GlobalSearchResponse,
  Pin,
  CreatePinRequest,
  UpdatePinRequest,
  ChannelPin,
  ServerSettings,
  OidcProvider,
  OidcLoginResult,
  AuthSettingsResponse,
  AuthMethodsConfig,
  AdminOidcProvider,
  GuildSettings,
  GuildUsageStats,
  DiscoverResponse,
  JoinDiscoverableResponse,
  PageRevision,
  RevisionListItem,
  PageCategory,
  SessionInfo,
  SessionListResponse,
  RevokeAllResponse,
} from "../types";

// Shared helpers and exported state/types from common.ts
export {
  fetchUploadLimits,
  getUploadLimitText,
  validateFileSize,
  fetchApi,
  getServerUrl,
  getAccessToken,
  HttpError,
  httpRequest,
  refreshAccessToken,
  type SessionRestoreResult,
  type AuthResult,
} from "./common";

// Domain command modules
export * from "./auth";
export * from "./channels";
export * from "./messages";
export * from "./guilds";
export * from "./forum";
export * from "./events";
export * from "./screening";
export * from "./voice";
export * from "./social";
export * from "./admin";
export * from "./settings";
export * from "./pages";
export * from "./e2ee";
export * from "./websocket";
export * from "./search";
export * from "./dms";
export * from "./webhooks";
