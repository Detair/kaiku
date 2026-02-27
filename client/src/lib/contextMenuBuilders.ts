/**
 * Context Menu Builders
 *
 * Reusable context menu item builders for common entities (users, etc.).
 */

import { User, MessageSquare, UserPlus, Ban, Copy, Flag } from "lucide-solid";
import {
  showContextMenu,
  type ContextMenuEntry,
} from "@/components/ui/ContextMenu";
import { currentUser } from "@/stores/auth";
import { createDM } from "@/lib/tauri";
import { selectDM, loadDMs, dmsState } from "@/stores/dms";
import { selectHome } from "@/stores/guilds";
import { sendFriendRequest } from "@/stores/friends";
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
 * Show a context menu for a user (member list, message author, etc.).
 */
export function showUserContextMenu(
  event: MouseEvent,
  user: UserMenuTarget,
): void {
  const me = currentUser();
  const isSelf = me?.id === user.id;

  const items: ContextMenuEntry[] = [
    {
      label: "View Profile",
      icon: User,
      action: async () => {
        // Navigate to DM with this user to see their profile info
        try {
          const existing = dmsState.dms.find(
            (dm) =>
              dm.participants?.some((p) => p.user_id === user.id) &&
              dm.participants.length <= 2,
          );
          if (existing) {
            selectHome();
            selectDM(existing.id);
          } else {
            const dm = await createDM([user.id]);
            await loadDMs();
            selectHome();
            selectDM(dm.channel.id);
          }
        } catch (e) {
          console.error("Failed to view profile:", e);
        }
      },
    },
  ];

  if (!isSelf) {
    items.push(
      {
        label: "Send Message",
        icon: MessageSquare,
        action: async () => {
          try {
            const existing = dmsState.dms.find(
              (dm) =>
                dm.participants?.some((p) => p.user_id === user.id) &&
                dm.participants.length <= 2,
            );
            if (existing) {
              selectHome();
              selectDM(existing.id);
            } else {
              const dm = await createDM([user.id]);
              await loadDMs();
              selectHome();
              selectDM(dm.channel.id);
            }
          } catch (e) {
            console.error("Failed to open DM:", e);
          }
        },
      },
      { separator: true },
      {
        label: "Add Friend",
        icon: UserPlus,
        action: async () => {
          try {
            await sendFriendRequest(user.username);
          } catch (e) {
            console.error("Failed to send friend request:", e);
          }
        },
      },
      { separator: true },
      {
        label: "Report",
        icon: Flag,
        danger: true,
        action: () => {
          if (showReportCallback) {
            showReportCallback({ userId: user.id, username: user.username });
          }
        },
      },
      {
        label: "Block",
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

  items.push(
    { separator: true },
    {
      label: "Copy User ID",
      icon: Copy,
      action: () => navigator.clipboard.writeText(user.id),
    },
  );

  showContextMenu(event, items);
}
