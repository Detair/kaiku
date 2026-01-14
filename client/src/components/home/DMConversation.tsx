/**
 * DMConversation Component
 *
 * Displays a DM conversation in the Home view.
 */

import { Component, Show, onCleanup, createEffect, createSignal } from "solid-js";
import { Phone } from "lucide-solid";
import { getSelectedDM, markDMAsRead } from "@/stores/dms";
import MessageList from "@/components/messages/MessageList";
import MessageInput from "@/components/messages/MessageInput";
import TypingIndicator from "@/components/messages/TypingIndicator";
import { CallBanner } from "@/components/call";
import { callState, startCall, isInCallForChannel } from "@/stores/call";
import { startDMCall } from "@/lib/tauri";

const DMConversation: Component = () => {
  const dm = () => getSelectedDM();
  const [isStartingCall, setIsStartingCall] = createSignal(false);

  const handleStartCall = async () => {
    const currentDM = dm();
    if (!currentDM) return;

    setIsStartingCall(true);
    try {
      startCall(currentDM.id);
      await startDMCall(currentDM.id);
    } catch (err) {
      console.error("Failed to start call:", err);
    } finally {
      setIsStartingCall(false);
    }
  };

  const canStartCall = () => {
    const currentDM = dm();
    if (!currentDM) return false;
    // Can't start a call if already in one for this channel
    if (isInCallForChannel(currentDM.id)) return false;
    // Can't start a call if in any call state except idle
    if (callState.currentCall.status !== "idle") return false;
    return true;
  };

  // Mark as read when viewing
  createEffect(() => {
    const currentDM = dm();
    if (currentDM && currentDM.unread_count > 0) {
      // Debounce: wait 1 second before marking as read
      const timer = setTimeout(() => {
        markDMAsRead(currentDM.id);
      }, 1000);
      onCleanup(() => clearTimeout(timer));
    }
  });

  const displayName = () => {
    const currentDM = dm();
    if (!currentDM) return "";
    if (currentDM.participants.length === 1) {
      return currentDM.participants[0].display_name;
    }
    return currentDM.name || currentDM.participants.map(p => p.display_name).join(", ");
  };

  const isGroupDM = () => {
    const currentDM = dm();
    return currentDM ? currentDM.participants.length > 1 : false;
  };

  return (
    <Show
      when={dm()}
      fallback={
        <div class="flex-1 flex items-center justify-center bg-surface-layer1">
          <p class="text-text-secondary">Select a conversation</p>
        </div>
      }
    >
      <div class="flex-1 flex flex-col bg-surface-layer1">
        {/* Header */}
        <header class="h-12 px-4 flex items-center gap-3 border-b border-white/5 bg-surface-layer1 shadow-sm">
          <Show
            when={isGroupDM()}
            fallback={
              <div class="w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center">
                <span class="text-sm font-semibold text-surface-base">
                  {dm()?.participants[0]?.display_name?.charAt(0).toUpperCase()}
                </span>
              </div>
            }
          >
            <div class="w-8 h-8 rounded-full bg-surface-layer2 flex items-center justify-center">
              <svg class="w-4 h-4 text-text-secondary" fill="currentColor" viewBox="0 0 20 20">
                <path d="M13 6a3 3 0 11-6 0 3 3 0 016 0zM18 8a2 2 0 11-4 0 2 2 0 014 0zM14 15a4 4 0 00-8 0v3h8v-3z" />
              </svg>
            </div>
          </Show>
          <span class="font-semibold text-text-primary">{displayName()}</span>
          <Show when={isGroupDM()}>
            <span class="text-sm text-text-secondary">
              {dm()?.participants.length} members
            </span>
          </Show>

          {/* Spacer */}
          <div class="flex-1" />

          {/* Call Button */}
          <button
            onClick={handleStartCall}
            disabled={!canStartCall() || isStartingCall()}
            title={canStartCall() ? "Start voice call" : "Call in progress"}
            class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-layer2 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Phone class="w-5 h-5" />
          </button>
        </header>

        {/* Call Banner */}
        <CallBanner channelId={dm()!.id} />

        {/* Messages */}
        <MessageList channelId={dm()!.id} />

        {/* Typing Indicator */}
        <TypingIndicator channelId={dm()!.id} />

        {/* Message Input */}
        <MessageInput channelId={dm()!.id} channelName={displayName()} />
      </div>
    </Show>
  );
};

export default DMConversation;
