import { Component, For, Show, createMemo } from "solid-js";
import { Crosshair } from "lucide-solid";
import { UserStatus } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { markManualStatusChange } from "@/stores/presence";
import {
  focusState,
  getActiveFocusMode,
  deactivateFocusMode,
} from "@/stores/focus";
import StatusIndicator from "./StatusIndicator";

interface StatusPickerProps {
  currentStatus: UserStatus;
  onClose: () => void;
  onCustomStatusClick?: () => void;
}

const STATUS_OPTIONS: { value: UserStatus; label: string }[] = [
  { value: "online", label: "Online" },
  { value: "idle", label: "Idle" },
  { value: "dnd", label: "Do Not Disturb" },
  { value: "invisible", label: "Invisible" },
];

const StatusPicker: Component<StatusPickerProps> = (props) => {
  const activeMode = createMemo(() => getActiveFocusMode());

  const handleSelect = async (status: UserStatus) => {
    try {
      markManualStatusChange(status);
      await tauri.updateStatus(status);
      props.onClose();
    } catch (err) {
      console.error("Failed to set status:", err);
    }
  };

  return (
    <div
      class="absolute bottom-full left-0 mb-2 w-48 bg-surface-layer2 border border-white/10 rounded-xl shadow-xl overflow-hidden animate-slide-up z-50"
      onClick={(e) => e.stopPropagation()}
    >
      <div class="p-1">
        <For each={STATUS_OPTIONS}>
          {(option) => (
            <button
              onClick={() => handleSelect(option.value)}
              class="w-full flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-white/5 transition-colors text-left group"
            >
              <div class="group-hover:scale-110 transition-transform">
                <StatusIndicator status={option.value} size="sm" />
              </div>
              <span class="text-sm font-medium text-text-primary">
                {option.label}
              </span>
              <Show when={props.currentStatus === option.value}>
                <div class="ml-auto w-1.5 h-1.5 bg-white rounded-full" />
              </Show>
            </button>
          )}
        </For>
        <Show when={props.onCustomStatusClick}>
          <div class="border-t border-white/10 mt-2 pt-2">
            <button
              class="w-full flex items-center gap-2 px-3 py-2 rounded-lg text-sm text-text-secondary hover:bg-white/5 transition-colors"
              onClick={() => {
                props.onCustomStatusClick?.();
                props.onClose();
              }}
            >
              <span>💬</span>
              <span>Set Custom Status...</span>
            </button>
          </div>
        </Show>
        <Show when={activeMode()}>
          {(mode) => (
            <div class="border-t border-white/10 mt-2 pt-2">
              <div class="flex items-center justify-between px-3 py-2">
                <div class="flex items-center gap-2">
                  <Crosshair class="w-3.5 h-3.5 text-accent-primary" />
                  <span class="text-xs text-accent-primary font-medium">
                    {mode().name}
                    {focusState().autoActivated ? " (auto)" : ""}
                  </span>
                </div>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    deactivateFocusMode();
                  }}
                  class="text-xs px-2 py-0.5 rounded bg-white/10 hover:bg-white/20 text-text-secondary transition-colors"
                >
                  End
                </button>
              </div>
            </div>
          )}
        </Show>
      </div>
    </div>
  );
};

export default StatusPicker;
