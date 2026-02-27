/**
 * Autocomplete Popup Component
 *
 * Data wrapper for PopupList that provides user, emoji, channel, and command autocomplete.
 * Handles data fetching and formatting for @user, :emoji:, #channel, and /command suggestions.
 */

import { Component, createMemo } from "solid-js";
import PopupList, { type PopupListItem } from "@/components/ui/PopupList";
import Avatar from "@/components/ui/Avatar";
import { Hash, Terminal } from "lucide-solid";
import { searchEmojis } from "@/lib/emojiData";
import { emojiState } from "@/stores/emoji";
import type { GuildMember } from "@/lib/types";
import type { ChannelWithUnread } from "@/lib/types";
import type { GuildCommand } from "@/lib/api/bots";

interface AutocompletePopupProps {
  /** Reference element to position relative to */
  anchorEl: HTMLElement;
  /** Type of autocomplete */
  type: "user" | "emoji" | "channel" | "command";
  /** Search query */
  query: string;
  /** Currently selected index */
  selectedIndex: number;
  /** Guild members (for @user autocomplete in guilds) */
  guildMembers?: GuildMember[];
  /** DM participants (for @user autocomplete in DMs) */
  dmParticipants?: Array<{
    user_id: string;
    username: string;
    display_name: string;
    avatar_url: string | null;
  }>;
  /** Optional guild ID for custom emojis */
  guildId?: string;
  /** Channels for #channel autocomplete */
  channels?: ChannelWithUnread[];
  /** Commands for /command autocomplete */
  commands?: GuildCommand[];
  /** Callback when an item is selected */
  onSelect: (value: string) => void;
  /** Callback when popup should close */
  onClose: () => void;
  /** Callback when selection changes */
  onSelectionChange: (index: number) => void;
}

const AutocompletePopup: Component<AutocompletePopupProps> = (props) => {
  // Get user suggestions
  const userItems = createMemo((): PopupListItem[] => {
    if (props.type !== "user") return [];

    const query = props.query.toLowerCase();
    let users: Array<{
      user_id: string;
      username: string;
      display_name: string;
      avatar_url: string | null;
    }> = [];

    // Get users from guild members or DM participants
    if (props.guildMembers) {
      users = props.guildMembers.map((m) => ({
        user_id: m.user_id,
        username: m.username,
        display_name: m.display_name,
        avatar_url: m.avatar_url,
      }));
    } else if (props.dmParticipants) {
      users = props.dmParticipants;
    }

    // Filter by query (match username or display name)
    const filtered = users.filter(
      (u) =>
        u.username.toLowerCase().includes(query) ||
        u.display_name.toLowerCase().includes(query),
    );

    // Sort to prioritize prefix matches
    filtered.sort((a, b) => {
      const aDisplayStartsWith = a.display_name.toLowerCase().startsWith(query);
      const aUsernameStartsWith = a.username.toLowerCase().startsWith(query);
      const bDisplayStartsWith = b.display_name.toLowerCase().startsWith(query);
      const bUsernameStartsWith = b.username.toLowerCase().startsWith(query);

      // Prioritize display name prefix matches first
      if (aDisplayStartsWith && !bDisplayStartsWith) return -1;
      if (!aDisplayStartsWith && bDisplayStartsWith) return 1;

      // Then prioritize username prefix matches
      if (aUsernameStartsWith && !bUsernameStartsWith) return -1;
      if (!aUsernameStartsWith && bUsernameStartsWith) return 1;

      // Otherwise maintain original order
      return 0;
    });

    // Limit to 8 results
    const limited = filtered.slice(0, 8);

    return limited.map((u) => ({
      id: u.user_id,
      label: u.display_name,
      description: `@${u.username}`,
      icon: <Avatar src={u.avatar_url} alt={u.display_name} size="sm" />,
    }));
  });

  // Get emoji suggestions
  const emojiItems = createMemo((): PopupListItem[] => {
    if (props.type !== "emoji") return [];

    const query = props.query;
    const results: PopupListItem[] = [];

    // Search standard emojis
    const standardEmojis = searchEmojis(query);
    results.push(
      ...standardEmojis.slice(0, 8).map((emoji) => ({
        id: emoji,
        label: emoji,
        description: undefined,
        icon: undefined,
      })),
    );

    // Search custom guild emojis
    if (props.guildId) {
      const guildEmojis = emojiState.guildEmojis[props.guildId] ?? [];
      const customMatches = guildEmojis
        .filter((e) => e.name.toLowerCase().includes(query.toLowerCase()))
        .slice(0, 8 - results.length);

      results.push(
        ...customMatches.map((e) => ({
          id: `:${e.name}:`,
          label: e.name,
          description: "Custom emoji",
          icon: <img src={e.image_url} alt={e.name} class="w-5 h-5" />,
        })),
      );
    }

    return results.slice(0, 8);
  });

  // Get channel suggestions
  const channelItems = createMemo((): PopupListItem[] => {
    if (props.type !== "channel") return [];

    const query = props.query.toLowerCase();
    const channels = props.channels ?? [];

    // Filter to text channels and match name against query
    const filtered = channels.filter(
      (c) => c.channel_type === "text" && c.name.toLowerCase().includes(query),
    );

    // Sort: prefix matches first, then by position
    filtered.sort((a, b) => {
      const aStartsWith = a.name.toLowerCase().startsWith(query);
      const bStartsWith = b.name.toLowerCase().startsWith(query);
      if (aStartsWith && !bStartsWith) return -1;
      if (!aStartsWith && bStartsWith) return 1;
      return a.position - b.position;
    });

    return filtered.slice(0, 8).map((c) => ({
      id: c.id,
      label: c.name,
      description: c.topic || undefined,
      icon: <Hash class="w-4 h-4 text-text-secondary" />,
    }));
  });

  // Get command suggestions
  const commandItems = createMemo((): PopupListItem[] => {
    if (props.type !== "command") return [];

    const query = props.query.toLowerCase();
    const commands = props.commands ?? [];

    // Filter by name matching query
    const filtered = commands.filter((c) =>
      c.name.toLowerCase().includes(query),
    );

    // Sort: prefix matches first
    filtered.sort((a, b) => {
      const aStartsWith = a.name.toLowerCase().startsWith(query);
      const bStartsWith = b.name.toLowerCase().startsWith(query);
      if (aStartsWith && !bStartsWith) return -1;
      if (!aStartsWith && bStartsWith) return 1;
      return 0;
    });

    return filtered.slice(0, 8).map((c) => ({
      id: c.is_ambiguous ? `${c.name}:${c.application_id}` : c.name,
      label: `/${c.name}`,
      description: c.is_ambiguous
        ? `${c.description} · ${c.bot_name} (ambiguous)`
        : `${c.description} · ${c.bot_name}`,
      icon: <Terminal class="w-4 h-4 text-text-secondary" />,
    }));
  });

  const items = () => {
    switch (props.type) {
      case "user":
        return userItems();
      case "emoji":
        return emojiItems();
      case "channel":
        return channelItems();
      case "command":
        return commandItems();
    }
  };

  const handleSelect = (item: PopupListItem) => {
    switch (props.type) {
      case "user":
        // For users, return the display name with @ prefix and trailing space
        props.onSelect(`@${item.label} `);
        break;
      case "emoji":
        // For emojis, return the emoji character or :name: for custom
        props.onSelect(item.id.startsWith(":") ? item.id : item.id);
        break;
      case "channel":
        // For channels, return #channel-name with trailing space
        props.onSelect(`#${item.label} `);
        break;
      case "command":
        // For commands, return /command-name with trailing space
        props.onSelect(`/${item.id} `);
        break;
    }
  };

  return (
    <PopupList
      anchorEl={props.anchorEl}
      items={items()}
      selectedIndex={props.selectedIndex}
      onSelect={handleSelect}
      onClose={props.onClose}
      onSelectionChange={props.onSelectionChange}
    />
  );
};

export default AutocompletePopup;
