/**
 * DMConversation Component
 *
 * Displays a DM conversation in the Home view.
 */

import { Component, Show, onCleanup, createEffect, createSignal } from "solid-js";
import { Phone, Lock, Unlock, Check, X } from "lucide-solid";
import { e2eeStatus } from "@/stores/e2ee";
import { getSelectedDM, markDMAsRead, updateDMIconUrl } from "@/stores/dms";
import { currentUser } from "@/stores/auth";
import MessageList from "@/components/messages/MessageList";
import MessageInput from "@/components/messages/MessageInput";
import TypingIndicator from "@/components/messages/TypingIndicator";
import { CallBanner } from "@/components/call";
import { callState, startCall, isInCallForChannel } from "@/stores/call";
import { startDMCall, joinVoice, uploadDMAvatar, updateDMName, validateFileSize, getUploadLimitText } from "@/lib/tauri";

const DMConversation: Component = () => {
  const dm = () => getSelectedDM();
  const [isStartingCall, setIsStartingCall] = createSignal(false);
  const [showEncryptionTooltip, setShowEncryptionTooltip] = createSignal(false);
  const [isEditingName, setIsEditingName] = createSignal(false);
  const [editName, setEditName] = createSignal("");
  const [uploadError, setUploadError] = createSignal<string | null>(null);

  // E2EE status for encryption indicator
  const isEncrypted = () => e2eeStatus().initialized;

  const handleStartCall = async () => {
    const currentDM = dm();
    if (!currentDM) return;

    setIsStartingCall(true);
    try {
      await startDMCall(currentDM.id);
      startCall(currentDM.id);
      // Start voice connection immediately as the initiator
      await joinVoice(currentDM.id);
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

  const otherParticipants = () => {
    const currentDM = dm();
    if (!currentDM) return [];
    const me = currentUser();
    return currentDM.participants.filter(p => p.user_id !== me?.id);
  };

  const displayName = () => {
    const currentDM = dm();
    if (!currentDM) return "";
    // For group DMs, use the channel name (custom or auto-generated)
    if (isGroupDM()) {
      return currentDM.name;
    }
    // For 1:1 DMs, show the other participant's display name
    const others = otherParticipants();
    if (others.length === 0) {
      return currentDM.participants[0]?.display_name ?? "Unknown";
    }
    return others[0].display_name;
  };

  const isGroupDM = () => otherParticipants().length > 1;

  const startEditingName = () => {
    setEditName(dm()?.name ?? displayName());
    setIsEditingName(true);
  };

  const saveEditedName = async () => {
    const currentDM = dm();
    const newName = editName().trim();
    if (!currentDM || !newName || newName === currentDM.name) {
      setIsEditingName(false);
      return;
    }
    try {
      await updateDMName(currentDM.id, newName);
      setIsEditingName(false);
    } catch (err) {
      console.error("Failed to update DM name:", err);
    }
  };

  const cancelEditingName = () => {
    setIsEditingName(false);
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
      <div class="flex-1 flex flex-col min-h-0 bg-surface-layer1">
        {/* Header */}
        <header class="h-12 px-4 flex items-center gap-3 border-b border-white/5 bg-surface-layer1 shadow-sm relative">
          <input
            type="file"
            accept="image/*"
            class="hidden"
            ref={(el) => {
              // @ts-ignore
              el.onchange = async (e: any) => {
                const file = e.target.files?.[0];
                if (file && dm()) {
                  setUploadError(null);

                  // Frontend validation
                  const validationError = validateFileSize(file, 'avatar');
                  if (validationError) {
                    setUploadError(validationError);
                    setTimeout(() => setUploadError(null), 5000);
                    e.target.value = ""; // Clear selection
                    return;
                  }

                  try {
                    const result = await uploadDMAvatar(dm()!.id, file);
                    updateDMIconUrl(dm()!.id, result.icon_url);
                  } catch (err) {
                    console.error("Failed to upload icon", err);
                    setUploadError("Failed to upload icon");
                    setTimeout(() => setUploadError(null), 3000);
                  }
                  e.target.value = ""; // Clear selection
                }
              };
            }}
            id="dm-avatar-upload"
          />
          <label
            for="dm-avatar-upload"
            class="w-8 h-8 rounded-full flex items-center justify-center cursor-pointer hover:opacity-80 transition-opacity overflow-hidden relative group"
            classList={{
              "bg-accent-primary": !dm()?.icon_url,
              "bg-surface-layer2": !!dm()?.icon_url,
            }}
            title={`Change icon (Max ${getUploadLimitText('avatar')})`}
          >
            <Show
              when={dm()?.icon_url}
              fallback={
                <span class="text-sm font-semibold text-white">
                  {isGroupDM()
                    ? displayName().charAt(0).toUpperCase()
                    : otherParticipants()[0]?.display_name?.charAt(0).toUpperCase()}
                </span>
              }
            >
              <img src={dm()!.icon_url!} alt="DM Icon" class="w-full h-full object-cover" />
            </Show>
            <div class="absolute inset-0 bg-black/40 hidden group-hover:flex items-center justify-center rounded-full">
              <svg class="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" />
              </svg>
            </div>
          </label>

          {/* Upload Error Tooltip/Toast */}
          <Show when={uploadError()}>
            <div class="absolute top-12 left-4 z-50 p-2 text-xs bg-error-bg text-error-text border border-error-border rounded shadow-lg whitespace-nowrap">
              {uploadError()}
            </div>
          </Show>

          {/* Editable name for group DMs */}
          <Show
            when={isGroupDM() && isEditingName()}
            fallback={
              <span
                class="font-semibold text-text-primary"
                classList={{ "cursor-pointer hover:underline": isGroupDM() }}
                onClick={() => isGroupDM() && startEditingName()}
                title={isGroupDM() ? "Click to rename" : undefined}
              >
                {displayName()}
              </span>
            }
          >
            <div class="flex items-center gap-1">
              <input
                type="text"
                value={editName()}
                onInput={(e) => setEditName(e.currentTarget.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") saveEditedName();
                  if (e.key === "Escape") cancelEditingName();
                }}
                class="px-2 py-0.5 rounded text-sm font-semibold text-text-primary bg-surface-layer2 border border-white/10 outline-none focus:ring-1 focus:ring-accent-primary/50 w-48"
                maxLength={100}
                autofocus
              />
              <button
                onClick={saveEditedName}
                class="p-1 rounded hover:bg-white/10 text-accent-success transition-colors"
                title="Save"
              >
                <Check class="w-4 h-4" />
              </button>
              <button
                onClick={cancelEditingName}
                class="p-1 rounded hover:bg-white/10 text-text-secondary transition-colors"
                title="Cancel"
              >
                <X class="w-4 h-4" />
              </button>
            </div>
          </Show>

          {/* Encryption Indicator */}
          <div
            class="relative"
            onMouseEnter={() => setShowEncryptionTooltip(true)}
            onMouseLeave={() => setShowEncryptionTooltip(false)}
          >
            <Show
              when={isEncrypted()}
              fallback={
                <Unlock
                  class="w-4 h-4 text-text-muted cursor-help"
                  aria-label="End-to-end encryption not active"
                />
              }
            >
              <Lock
                class="w-4 h-4 text-accent-primary cursor-help"
                aria-label="End-to-end encryption active"
              />
            </Show>

            {/* Encryption Status Tooltip */}
            <Show when={showEncryptionTooltip()}>
              <div class="absolute left-1/2 -translate-x-1/2 top-full mt-2 z-50 px-3 py-2 bg-surface-base border border-white/10 rounded-lg shadow-xl min-w-[200px] text-center">
                <Show
                  when={isEncrypted()}
                  fallback={
                    <>
                      <p class="text-sm font-medium text-text-secondary">
                        Not Encrypted
                      </p>
                      <p class="text-xs text-text-muted mt-1">
                        E2EE is not set up. Messages are not end-to-end encrypted.
                      </p>
                    </>
                  }
                >
                  <p class="text-sm font-medium text-accent-primary">
                    End-to-End Encrypted
                  </p>
                  <p class="text-xs text-text-muted mt-1">
                    Messages are secured with end-to-end encryption. Only you and the recipient can read them.
                  </p>
                </Show>
              </div>
            </Show>
          </div>

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
            aria-label={canStartCall() ? "Start voice call" : "Call in progress"}
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
