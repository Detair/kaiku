/**
 * Context Menu Builders
 *
 * Reusable context menu item builders for common entities (users, etc.).
 */

import { User, MessageSquare, UserPlus, Ban, Copy, Flag } from "lucide-solid";
import { showContextMenu, type ContextMenuEntry } from "@/components/ui/ContextMenu";
import { currentUser } from "@/stores/auth";
interface UserMenuTarget {
  id: string;
  username: string;
  display_name?: string;
}

// Block confirm state (used by BlockConfirmModal)
let pendingBlockTarget: UserMenuTarget | null = null;
let showBlockConfirmCallback: ((target: UserMenuTarget) => void) | null = null;
let showReportCallback: ((target: { userId: string; username: string; messageId?: string }) => void) | null = null;

/**
 * Register callback to show the block confirmation modal.
 */
export function onShowBlockConfirm(callback: (target: UserMenuTarget) => void): void {
  showBlockConfirmCallback = callback;
}

/**
 * Register callback to show the report modal.
 */
export function onShowReport(callback: (target: { userId: string; username: string; messageId?: string }) => void): void {
  showReportCallback = callback;
}

/**
 * Get the pending block target (for modal use).
 */
export function getPendingBlockTarget(): UserMenuTarget | null {
  return pendingBlockTarget;
}

/**
 * Show a context menu for a user (member list, message author, etc.).
 */
export function showUserContextMenu(event: MouseEvent, user: UserMenuTarget): void {
  const me = currentUser();
  const isSelf = me?.id === user.id;

  const items: ContextMenuEntry[] = [
    {
      label: "View Profile",
      icon: User,
      action: () => {
        // TODO: open profile modal/panel
        console.log("View profile:", user.id);
      },
    },
  ];

  if (!isSelf) {
    items.push(
      {
        label: "Send Message",
        icon: MessageSquare,
        action: () => {
          // TODO: navigate to or create DM with this user
          console.log("Send message to:", user.id);
        },
      },
      { separator: true },
      {
        label: "Add Friend",
        icon: UserPlus,
        action: () => {
          // TODO: send friend request
          console.log("Add friend:", user.username);
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
