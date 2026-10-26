/**
 * DMConversation Component
 *
 * Displays a DM conversation in the Home view.
 */

import {
  Component,
  Show,
  createSignal,
} from "solid-js";
import { Phone, Check, X } from "lucide-solid";
import { t, plural } from "@/i18n";
import { errorText, CALL_START_ERRORS } from "@/i18n/errors";
import { getSelectedDM, updateDMIconUrl } from "@/stores/dms";
import { currentUser } from "@/stores/auth";
import MessageList from "@/components/messages/MessageList";
import MessageInput from "@/components/messages/MessageInput";
import TypingIndicator from "@/components/messages/TypingIndicator";
import { showToast } from "@/components/ui/Toast";
import { CallBanner } from "@/components/call";
import { callState, startCall, endCall, isInCallForChannel } from "@/stores/call";
import {
  startDMCall,
  joinVoice,
  uploadDMAvatar,
  updateDMName,
  validateFileSize,
  getUploadLimitText,
} from "@/lib/tauri";

const DMConversation: Component = () => {
  const dm = () => getSelectedDM();
  const [isStartingCall, setIsStartingCall] = createSignal(false);
  const [isEditingName, setIsEditingName] = createSignal(false);
  const [editName, setEditName] = createSignal("");
  const [uploadError, setUploadError] = createSignal<string | null>(null);

  const handleStartCall = async () => {
    const currentDM = dm();
    if (!currentDM) return;

    setIsStartingCall(true);
    // Set outgoing state BEFORE the HTTP call so the WebSocket
    // incoming_call echo (which arrives before the response) is
    // correctly ignored by the call store.
    startCall(currentDM.id);
    try {
      await startDMCall(currentDM.id);
      await joinVoice(currentDM.id);
    } catch (err) {
      console.error("Failed to start call:", err);
      showToast({
        type: "error",
        title: errorText(err, "voice.call.startFailed", CALL_START_ERRORS.byCode),
        duration: 5000,
      });
      // Roll back optimistic call state on failure
      endCall(currentDM.id, "cancelled");
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

  const selectedChannelId = () => dm()?.id;

  const otherParticipants = () => {
    const currentDM = dm();
    if (!currentDM) return [];
    const me = currentUser();
    return currentDM.participants.filter((p) => p.user_id !== me?.id);
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
      return currentDM.participants[0]?.display_name ?? t("dm.header.unknownUser");
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
      showToast({
        type: "error",
        title: t("dm.header.renameFailed"),
        duration: 5000,
      });
      setIsEditingName(false);
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
          <p class="text-text-secondary">{t("dm.header.selectConversation")}</p>
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
            ref={(el: HTMLInputElement) => {
              el.onchange = (e: Event) => {
                const input = e.target as HTMLInputElement;
                const file = input.files?.[0];
                const currentDm = dm();
                if (file && currentDm) {
                  setUploadError(null);

                  // Frontend validation
                  const validationError = validateFileSize(file, "avatar");
                  if (validationError) {
                    setUploadError(validationError);
                    setTimeout(() => setUploadError(null), 5000);
                    input.value = ""; // Clear selection
                    return;
                  }

                  const dmId = currentDm.id;
                  uploadDMAvatar(dmId, file)
                    .then((result) => {
                      updateDMIconUrl(dmId, result.icon_url);
                    })
                    .catch((err) => {
                      console.error("Failed to upload icon", err);
                      setUploadError(t("dm.header.uploadIconFailed"));
                      setTimeout(() => setUploadError(null), 3000);
                    })
                    .finally(() => {
                      input.value = ""; // Clear selection
                    });
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
            title={t("dm.header.changeIcon", { limit: getUploadLimitText("avatar") })}
          >
            <Show
              when={dm()?.icon_url}
              fallback={
                <span class="text-sm font-semibold text-white">
                  {isGroupDM()
                    ? displayName().charAt(0).toUpperCase()
                    : otherParticipants()[0]
                        ?.display_name?.charAt(0)
                        .toUpperCase()}
                </span>
              }
            >
              <img
                src={dm()!.icon_url!}
                alt={t("dm.header.iconAlt")}
                class="w-full h-full object-cover"
              />
            </Show>
            <div class="absolute inset-0 bg-black/40 hidden group-hover:flex items-center justify-center rounded-full">
              <svg
                class="w-3 h-3 text-white"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z"
                />
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
                title={isGroupDM() ? t("dm.header.clickToRename") : undefined}
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
                title={t("common.save")}
              >
                <Check class="w-4 h-4" />
              </button>
              <button
                onClick={cancelEditingName}
                class="p-1 rounded hover:bg-white/10 text-text-secondary transition-colors"
                title={t("common.cancel")}
              >
                <X class="w-4 h-4" />
              </button>
            </div>
          </Show>

          <Show when={isGroupDM()}>
            <span class="text-sm text-text-secondary">
              {plural(dm()?.participants.length ?? 0, {
                one: "dm.header.members.one",
                other: "dm.header.members.other",
              })}
            </span>
          </Show>

          {/* Spacer */}
          <div class="flex-1" />

          {/* Call Button */}
          <button
            onClick={handleStartCall}
            disabled={!canStartCall() || isStartingCall()}
            title={
              canStartCall()
                ? t("dm.header.startCall")
                : t("dm.header.callInProgress")
            }
            aria-label={
              canStartCall()
                ? t("dm.header.startCall")
                : t("dm.header.callInProgress")
            }
            class="p-2 rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-layer2 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Phone class="w-5 h-5" />
          </button>
        </header>

        <Show when={selectedChannelId()}>
          {(channelId) => (
            <>
              <CallBanner channelId={channelId()} />
              <MessageList channelId={channelId()} />
              <TypingIndicator channelId={channelId()} />
              <MessageInput
                channelId={channelId()}
                channelName={displayName()}
                dmParticipants={
                  dm()?.participants.map((p) => ({
                    user_id: p.user_id,
                    username: p.username,
                    display_name: p.display_name,
                    avatar_url: p.avatar_url,
                  })) ?? []
                }
              />
            </>
          )}
        </Show>
      </div>
    </Show>
  );
};

export default DMConversation;
