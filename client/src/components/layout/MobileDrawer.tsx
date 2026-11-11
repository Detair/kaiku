/**
 * MobileDrawer - Slide-out Navigation Drawer
 *
 * A left-edge drawer containing the server rail and sidebar for mobile viewports.
 * Supports swipe-left-to-close and backdrop click dismissal.
 * Prevents body scroll while open.
 */

import { Component, JSX, createEffect, onCleanup } from "solid-js";
import { X } from "lucide-solid";
import { t } from "@/i18n";

interface MobileDrawerProps {
  open: boolean;
  onClose: () => void;
  children: JSX.Element;
}

const MobileDrawer: Component<MobileDrawerProps> = (props) => {
  let startX = 0;
  let savedOverflow: string | null = null;

  // Swipe-left to close
  const onPointerDown = (e: PointerEvent) => {
    startX = e.clientX;
  };
  const onPointerUp = (e: PointerEvent) => {
    if (startX - e.clientX > 50) props.onClose();
  };

  // Prevent body scroll when drawer is open; save/restore prior overflow
  createEffect(() => {
    if (props.open && savedOverflow === null) {
      // Closed → open: snapshot the prior value once
      savedOverflow = document.body.style.overflow;
      document.body.style.overflow = "hidden";
    } else if (!props.open && savedOverflow !== null) {
      // Open → closed: restore and clear the snapshot
      document.body.style.overflow = savedOverflow;
      savedOverflow = null;
    }
  });

  // Escape closes an open drawer (NAV-12)
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape" && props.open && !e.defaultPrevented) {
      props.onClose();
    }
  };
  document.addEventListener("keydown", onKeyDown);
  onCleanup(() => document.removeEventListener("keydown", onKeyDown));

  onCleanup(() => {
    // Component disposed mid-lock: restore so we don't leave the page locked
    if (savedOverflow !== null) {
      document.body.style.overflow = savedOverflow;
      savedOverflow = null;
    }
  });

  return (
    <div
      data-drawer-open={props.open ? "true" : "false"}
      class="fixed inset-0 z-50"
      classList={{ "pointer-events-none": !props.open }}
      inert={!props.open ? true : undefined}
    >
      {/* Backdrop */}
      <div
        class="absolute inset-0 bg-black/50 transition-opacity duration-200"
        classList={{
          "opacity-100": props.open,
          "opacity-0 pointer-events-none": !props.open,
        }}
        onClick={() => props.onClose()}
      />

      {/* Drawer panel: a slim top bar holds the close button, so it is on
          screen at every width (320 px too) and covers nothing (N5) */}
      <div
        class="absolute top-0 left-0 h-full w-[300px] max-w-[100vw] flex flex-col transition-transform duration-200 bg-surface-base"
        classList={{
          "translate-x-0": props.open,
          "-translate-x-full": !props.open,
        }}
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
      >
        <div class="flex justify-end flex-shrink-0 border-b border-border-subtle">
            <button
              type="button"
              data-testid="mobile-drawer-close"
              class="w-11 h-11 flex items-center justify-center text-text-primary hover:bg-white/10"
              aria-label={t("common.close")}
              onClick={() => props.onClose()}
            >
              <X class="w-5 h-5" aria-hidden="true" />
            </button>
        </div>
        <div class="flex flex-1 min-h-0">{props.children}</div>
      </div>
    </div>
  );
};

export default MobileDrawer;
