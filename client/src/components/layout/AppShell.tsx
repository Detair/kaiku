/**
 * AppShell - Main Layout Grid
 *
 * The primary layout structure for Canis.
 * Implements "The Focused Hybrid" design philosophy:
 * Discord structure + Linear/Arc efficiency.
 *
 * Layout Structure:
 * 1. Server Rail (72px) - Leftmost vertical bar for server/guild navigation
 * 2. Context Sidebar (240px) - Channel list and user panel
 * 3. Main Stage (flex-1) - Chat messages and content
 * 4. Voice Island (overlay) - Floating voice controls
 */

import { Component, JSX, ParentProps, Show, lazy, Suspense } from "solid-js";
import ServerRail from "./ServerRail";
import Sidebar from "./Sidebar";
import VoiceIsland from "./VoiceIsland";
import { voiceState } from "@/stores/voice";
import { LazyErrorBoundary } from "@/components/ui/LazyFallback";

const ScreenShareViewer = lazy(
  () => import("@/components/voice/ScreenShareViewer"),
);

interface AppShellProps extends ParentProps {
  /**
   * Whether to show the server rail (for guild/server switching).
   * Currently hidden as guilds are not yet implemented.
   */
  showServerRail?: boolean;
  /**
   * Optional custom sidebar component to replace the default guild Sidebar.
   */
  sidebar?: JSX.Element;
}

const AppShell: Component<AppShellProps> = (props) => {
  const showServerRail = () => props.showServerRail ?? false;

  return (
    <div class="flex h-screen w-full bg-surface-base overflow-hidden selection:bg-accent-primary/30">
      {/* 1. Server Rail (Leftmost) */}
      <Show when={showServerRail()}>
        <ServerRail />
      </Show>

      {/* 2. Context Sidebar (Middle-Left) */}
      <Show when={props.sidebar} fallback={<Sidebar />}>
        {props.sidebar}
      </Show>

      {/* 3. Main Stage (Right) */}
      <main class="flex-1 flex flex-col min-w-0 bg-surface-layer1 relative border-l border-white/10">
        {/* Main content passed as children */}
        {props.children}
      </main>

      {/* 4. Dynamic Voice Island (Draggable Overlay) */}
      <Show when={voiceState.channelId}>
        <VoiceIsland />
      </Show>

      {/* Screen Share Viewer (Portal overlay) */}
      <LazyErrorBoundary name="ScreenShareViewer">
        <Suspense>
          <ScreenShareViewer />
        </Suspense>
      </LazyErrorBoundary>
    </div>
  );
};

export default AppShell;
