/**
 * Context Menu Builders
 *
 * Reusable context menu item builders for common entities (users, etc.).
 */

import { MessageSquare, UserPlus, Ban, Copy, Flag } from "lucide-solid";
import {
  showContextMenu,
  showContextMenuAt,
  type ContextMenuEntry,
} from "@/components/ui/ContextMenu";
import { currentUser } from "@/stores/auth";
import { openDirectMessage } from "@/stores/dms";
import { sendFriendRequest } from "@/stores/friends";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";
interface UserMenuTarget {
  id: string;
  username: string;
  display_name?: string;
}

// Block confirm state (used by BlockConfirmModal)
let pendingBlockTarget: UserMenuTarget | null = null;
let showBlockConfirmCallback: ((target: UserMenuTarget) => void) | null = null;
let showReportCallback:
  | ((target: { userId: string; username: string; messageId?: string }) => void)
  | null = null;

/**
 * Register callback to show the block confirmation modal.
 */
export function onShowBlockConfirm(
  callback: (target: UserMenuTarget) => void,
): void {
  showBlockConfirmCallback = callback;
}

/**
 * Register callback to show the report modal.
 */
export function onShowReport(
  callback: (target: {
    userId: string;
    username: string;
    messageId?: string;
  }) => void,
): void {
  showReportCallback = callback;
}

/**
 * Get the pending block target (for modal use).
 */
export function getPendingBlockTarget(): UserMenuTarget | null {
  return pendingBlockTarget;
}

/**
 * Trigger the report modal programmatically (e.g. from message context menu).
 */
export function triggerReport(target: {
  userId: string;
  username: string;
  messageId?: string;
}): void {
  if (showReportCallback) {
    showReportCallback(target);
  }
}

/**
 * Build context menu items for a user (member list, message author, etc.).
 */
function buildUserContextMenuItems(user: UserMenuTarget): ContextMenuEntry[] {
  const me = currentUser();
  const isSelf = me?.id === user.id;

  // "View Profile" used to just open a DM; it duplicated "Send Message"
  // under a misleading name, so it is gone (NAV-25).
  const items: ContextMenuEntry[] = [];

  if (!isSelf) {
    items.push(
      {
        label: t("dm.userMenu.sendMessage"),
        icon: MessageSquare,
        action: async () => {
          try {
            await openDirectMessage([user.id]);
          } catch (e) {
            console.error("Failed to open DM:", e);
            showToast({
              type: "error",
              title: t("home.friends.toast.messageFailedTitle"),
              message: t("home.friends.toast.messageFailed"),
              duration: 8000,
            });
          }
        },
      },
      { separator: true },
      {
        label: t("dm.userMenu.addFriend"),
        icon: UserPlus,
        action: async () => {
          try {
            await sendFriendRequest(user.username);
            showToast({
              type: "success",
              title: t("dm.userMenu.requestSentTitle"),
              message: t("dm.userMenu.requestSent", {
                name: user.display_name || user.username,
              }),
              duration: 3000,
            });
          } catch (e) {
            // The friends store already shows the failure.
            console.error("Failed to send friend request:", e);
          }
        },
      },
      { separator: true },
      {
        label: t("dm.userMenu.report"),
        icon: Flag,
        danger: true,
        action: () => {
          if (showReportCallback) {
            showReportCallback({ userId: user.id, username: user.username });
          }
        },
      },
      {
        label: t("dm.userMenu.block"),
        icon: Ban,
        danger: true,
        action: () => {
          pendingBlockTarget = user;
          if (showBlockConfirmCallback) {
            showBlockConfirmCallback(user);
          }
        },
      },
    );
  }

  if (items.length > 0) items.push({ separator: true });
  items.push(
    {
      label: t("dm.userMenu.copyId"),
      icon: Copy,
      action: () => navigator.clipboard.writeText(user.id),
    },
  );

  return items;
}

/**
 * Show a context menu for a user (member list, message author, etc.).
 */
export function showUserContextMenu(
  event: MouseEvent,
  user: UserMenuTarget,
): void {
  showContextMenu(event, buildUserContextMenuItems(user));
}

/**
 * Show a context menu for a user at specific coordinates (for touch/long-press).
 */
export function showUserContextMenuAt(
  x: number,
  y: number,
  user: UserMenuTarget,
): void {
  showContextMenuAt(x, y, buildUserContextMenuItems(user));
}
