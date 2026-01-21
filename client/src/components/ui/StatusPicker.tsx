import { Component, For, Show } from "solid-js";
import { UserStatus } from "@/lib/types";
import * as tauri from "@/lib/tauri";

interface StatusPickerProps {
  currentStatus: UserStatus;
  onClose: () => void;
}

const STATUS_OPTIONS: { value: UserStatus; label: string; color: string }[] = [
  { value: "online", label: "Online", color: "bg-green-500" },
  { value: "away", label: "Away", color: "bg-yellow-500" },
  { value: "busy", label: "Do Not Disturb", color: "bg-red-500" },
  { value: "offline", label: "Invisible", color: "bg-gray-500" },
];

const StatusPicker: Component<StatusPickerProps> = (props) => {
  const handleSelect = async (status: UserStatus) => {
    try {
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
              <div class={`w-3 h-3 rounded-full ${option.color} group-hover:scale-110 transition-transform`} />
              <span class="text-sm font-medium text-text-primary">
                {option.label}
              </span>
              <Show when={props.currentStatus === option.value}>
                <div class="ml-auto w-1.5 h-1.5 bg-white rounded-full" />
              </Show>
            </button>
          )}
        </For>
      </div>
    </div>
  );
};

export default StatusPicker;
