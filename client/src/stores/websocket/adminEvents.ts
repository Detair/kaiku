/**
 * Admin-related WebSocket event handlers: user bans, guild suspensions,
 * deletions, and report notifications.
 */

export async function handleAdminUserBanned(
  userId: string,
  username: string,
): Promise<void> {
  const { handleUserBannedEvent } = await import("@/stores/admin");
  handleUserBannedEvent(userId, username);
}

export async function handleAdminUserUnbanned(
  userId: string,
  username: string,
): Promise<void> {
  const { handleUserUnbannedEvent } = await import("@/stores/admin");
  handleUserUnbannedEvent(userId, username);
}

export async function handleAdminGuildSuspended(
  guildId: string,
  guildName: string,
): Promise<void> {
  const { handleGuildSuspendedEvent } = await import("@/stores/admin");
  handleGuildSuspendedEvent(guildId, guildName);
}

export async function handleAdminGuildUnsuspended(
  guildId: string,
  guildName: string,
): Promise<void> {
  const { handleGuildUnsuspendedEvent } = await import("@/stores/admin");
  handleGuildUnsuspendedEvent(guildId, guildName);
}

export async function handleAdminUserDeleted(
  userId: string,
  username: string,
): Promise<void> {
  const { handleUserDeletedEvent } = await import("@/stores/admin");
  handleUserDeletedEvent(userId, username);
}

export async function handleAdminGuildDeleted(
  guildId: string,
  guildName: string,
): Promise<void> {
  const { handleGuildDeletedEvent } = await import("@/stores/admin");
  handleGuildDeletedEvent(guildId, guildName);
}

// Admin report event handlers

export async function handleAdminReportCreated(
  reportId: string,
  category: string,
  targetType: string,
): Promise<void> {
  const { handleReportCreatedEvent } = await import("@/stores/admin");
  handleReportCreatedEvent(reportId, category, targetType);
}

export async function handleAdminReportResolved(reportId: string): Promise<void> {
  const { handleReportResolvedEvent } = await import("@/stores/admin");
  handleReportResolvedEvent(reportId);
}
