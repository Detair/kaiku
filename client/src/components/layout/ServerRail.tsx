/**
 * ServerRail - Leftmost Vertical Navigation
 *
 * Discord-inspired server switcher with enhanced animations.
 *
 * Visual Behavior:
 * - Default: Circular avatars (rounded-full), opacity-80
 * - Hover: Transition to rounded-[16px], opacity-100
 * - Active: rounded-[16px] + white "pill" indicator on left
 *
 * Structure:
 * - Top: "Canis Home" logo (Unified Home dashboard)
 * - Middle: Server/Guild icons (scrollable)
 * - Bottom: "Add Server" (+) button
 */

import { Component, createSignal, For, Show } from "solid-js";
import { Home, Plus } from "lucide-solid";
import { guildsState, selectHome, selectGuild } from "@/stores/guilds";
import CreateGuildModal from "@/components/guilds/CreateGuildModal";

const ServerRail: Component = () => {
  // Hover state (still local to component)
  const [hoveredServerId, setHoveredServerId] = createSignal<string | null>(null);
  const [showCreateModal, setShowCreateModal] = createSignal(false);

  // Active server comes from guild store
  const isActive = (id: string) => {
    if (id === "home") return guildsState.activeGuildId === null;
    return guildsState.activeGuildId === id;
  };

  const isHovered = (id: string) => hoveredServerId() === id;

  /**
   * Calculate border radius based on hover/active state
   */
  const getBorderRadius = (id: string) => {
    return isActive(id) || isHovered(id) ? "16px" : "50%";
  };

  /**
   * Calculate pill indicator height based on active state
   */
  const getPillHeight = (id: string) => {
    return isActive(id) ? "40px" : "8px";
  };

  return (
    <aside class="w-[72px] flex flex-col items-center py-3 gap-2 bg-surface-base border-r border-white/5 z-20">
      {/* Home Icon - Canis Logo */}
      <div class="relative">
        {/* Pill Indicator */}
        <div
          class="absolute -left-3 top-1/2 -translate-y-1/2 w-1 bg-white rounded-r-full transition-all duration-200"
          style={{ height: getPillHeight("home") }}
        />

        {/* Icon Container */}
        <button
          class="w-12 h-12 flex items-center justify-center bg-surface-layer2 transition-all duration-200 cursor-pointer"
          style={{
            "border-radius": getBorderRadius("home"),
            opacity: isActive("home") || isHovered("home") ? 1 : 0.8,
          }}
          onMouseEnter={() => setHoveredServerId("home")}
          onMouseLeave={() => setHoveredServerId(null)}
          onClick={() => selectHome()}
          title="Home"
        >
          <Home class="w-6 h-6 text-accent-primary" />
        </button>
      </div>

      {/* Separator */}
      <div class="w-8 h-0.5 bg-white/10 rounded-full my-1" />

      {/* Server Icons - Scrollable List */}
      <div class="flex-1 flex flex-col items-center gap-2 overflow-y-auto scrollbar-none">
        <For each={guildsState.guilds}>
          {(guild) => {
            // Compute initials from guild name (e.g., "Gaming Squad" -> "GS")
            const initials = guild.name
              .split(" ")
              .map((word) => word[0])
              .join("")
              .toUpperCase()
              .slice(0, 2);

            return (
              <div class="relative">
                {/* Pill Indicator */}
                <div
                  class="absolute -left-3 top-1/2 -translate-y-1/2 w-1 bg-white rounded-r-full transition-all duration-200"
                  style={{ height: getPillHeight(guild.id) }}
                />

                {/* Server Icon */}
                <button
                  class="w-12 h-12 flex items-center justify-center bg-surface-layer2 transition-all duration-200 cursor-pointer overflow-hidden"
                  style={{
                    "border-radius": getBorderRadius(guild.id),
                    opacity: isActive(guild.id) || isHovered(guild.id) ? 1 : 0.8,
                  }}
                  onMouseEnter={() => setHoveredServerId(guild.id)}
                  onMouseLeave={() => setHoveredServerId(null)}
                  onClick={() => selectGuild(guild.id)}
                  title={guild.name}
                >
                  {guild.icon_url ? (
                    <img
                      src={guild.icon_url}
                      alt={guild.name}
                      class="w-full h-full object-cover"
                    />
                  ) : (
                    <span class="text-sm font-semibold text-text-primary">
                      {initials}
                    </span>
                  )}
                </button>
              </div>
            );
          }}
        </For>
      </div>

      {/* Add Server Button */}
      <div class="relative">
        <button
          class="w-12 h-12 flex items-center justify-center bg-surface-layer2 hover:bg-accent-primary/20 transition-all duration-200 cursor-pointer group"
          style={{
            "border-radius": isHovered("add") ? "16px" : "50%",
            opacity: isHovered("add") ? 1 : 0.8,
          }}
          onMouseEnter={() => setHoveredServerId("add")}
          onMouseLeave={() => setHoveredServerId(null)}
          onClick={() => setShowCreateModal(true)}
          title="Add Server"
        >
          <Plus class="w-6 h-6 text-accent-primary transition-transform duration-200 group-hover:rotate-90" />
        </button>
      </div>

      {/* Create Guild Modal */}
      <Show when={showCreateModal()}>
        <CreateGuildModal onClose={() => setShowCreateModal(false)} />
      </Show>
    </aside>
  );
};

export default ServerRail;
