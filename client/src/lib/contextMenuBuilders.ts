/**
 * Context Menu Builders
 *
 * Reusable context menu item builders for common entities (users, etc.).
 */

import { User, MessageSquare, UserPlus, Ban, Copy } from "lucide-solid";
import { showContextMenu, type ContextMenuEntry } from "@/components/ui/ContextMenu";
import { currentUser } from "@/stores/auth";

interface UserMenuTarget {
  id: string;
  username: string;
  display_name?: string;
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
        label: "Block",
        icon: Ban,
        danger: true,
        action: () => {
          // TODO: block user
          console.log("Block:", user.id);
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
