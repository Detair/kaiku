/**
 * MobileDrawer - Slide-out Navigation Drawer
 *
 * A left-edge drawer containing the server rail and sidebar for mobile viewports.
 * Supports swipe-left-to-close and backdrop click dismissal.
 * Prevents body scroll while open.
 */

import { Component, JSX, Show, createEffect, onCleanup } from "solid-js";
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

      {/* Drawer panel */}
      <div
        class="absolute top-0 left-0 h-full w-[300px] flex transition-transform duration-200 bg-surface-base"
        classList={{
          "translate-x-0": props.open,
          "-translate-x-full": !props.open,
        }}
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
      >
        {props.children}
      </div>

      {/* Close button on the dimmed area next to the drawer (NAV-12) */}
      <Show when={props.open}>
        <button
          type="button"
          data-testid="mobile-drawer-close"
          class="absolute top-2 left-[308px] w-11 h-11 flex items-center justify-center rounded-full bg-surface-base/90 text-text-primary"
          aria-label={t("common.close")}
          onClick={() => props.onClose()}
        >
          <X class="w-5 h-5" aria-hidden="true" />
        </button>
      </Show>
    </div>
  );
};

export default MobileDrawer;
