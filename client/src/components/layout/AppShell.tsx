/**
 * AppShell - Main Layout Grid
 *
 * The primary layout structure for Kaiku.
 * Implements "The Focused Hybrid" design philosophy:
 * Discord structure + Linear/Arc efficiency.
 *
 * Layout Structure (Desktop):
 * 1. Server Rail (72px) - Leftmost vertical bar for server/guild navigation
 * 2. Context Sidebar (240px) - Channel list and user panel
 * 3. Main Stage (flex-1) - Chat messages and content
 *
 * Layout Structure (Mobile):
 * 1. MobileHeader (44px) - Top bar with hamburger menu + guild/channel name
 * 2. Main Stage (flex-1) - Chat messages and content
 * 3. MobileDrawer (overlay) - Server Rail (compact) + Sidebar, slides from left
 */

import {
  Component,
  JSX,
  ParentProps,
  Show,
  lazy,
  Suspense,
  createSignal,
} from "solid-js";
import { useIsMobile } from "@/lib/useBreakpoint";
import ServerRail from "./ServerRail";
import Sidebar from "./Sidebar";
import MobileDrawer from "./MobileDrawer";
import MobileHeader from "./MobileHeader";
import { LazyErrorBoundary } from "@/components/ui/LazyFallback";

const ScreenShareViewer = lazy(
  () => import("@/components/voice/ScreenShareViewer"),
);

interface AppShellProps extends ParentProps {
  /** Whether to show the server rail (for guild/server switching). */
  showServerRail?: boolean;
  /**
   * Optional custom sidebar component to replace the default guild Sidebar.
   */
  sidebar?: JSX.Element;
}

const AppShell: Component<AppShellProps> = (props) => {
  const showServerRail = () => props.showServerRail ?? false;
  const isMobile = useIsMobile();
  const [drawerOpen, setDrawerOpen] = createSignal(false);

  const closeDrawer = () => setDrawerOpen(false);

  // Swipe-right-to-open on the content area's left edge
  let edgeStartX: number | null = null;
  const onEdgePointerDown = (e: PointerEvent) => {
    if (isMobile() && e.clientX < 20) edgeStartX = e.clientX;
  };
  const onEdgePointerUp = (e: PointerEvent) => {
    if (edgeStartX !== null && e.clientX - edgeStartX > 50)
      setDrawerOpen(true);
    edgeStartX = null;
  };
  const onEdgeReset = () => {
    edgeStartX = null;
  };

  /** Sidebar content — shared between desktop inline and mobile drawer */
  const sidebarContent = (onNavigate?: () => void) => (
    <Show when={props.sidebar} fallback={<Sidebar onNavigate={onNavigate} />}>
      {props.sidebar}
    </Show>
  );

  return (
    <div
      class="flex h-screen w-full bg-surface-base overflow-hidden selection:bg-accent-primary/30"
      classList={{ "flex-col": isMobile() }}
      onPointerDown={onEdgePointerDown}
      onPointerUp={onEdgePointerUp}
      onPointerCancel={onEdgeReset}
      onPointerLeave={onEdgeReset}
    >
      <Show
        when={!isMobile()}
        fallback={
          <>
            {/* Mobile: Drawer + Header */}
            <MobileDrawer open={drawerOpen()} onClose={closeDrawer}>
              <Show when={showServerRail()}>
                <ServerRail compact />
              </Show>
              {sidebarContent(closeDrawer)}
            </MobileDrawer>

            <MobileHeader onMenuClick={() => setDrawerOpen(true)} />
          </>
        }
      >
        {/* Desktop: Inline Server Rail + Sidebar */}
        <Show when={showServerRail()}>
          <ServerRail />
        </Show>
        {sidebarContent()}
      </Show>

      {/* Main Stage */}
      <main class="flex-1 flex flex-col min-w-0 bg-surface-layer1 relative border-l border-border-solid">
        {props.children}
      </main>

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
