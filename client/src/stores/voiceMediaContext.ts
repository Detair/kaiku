/**
 * The channel voice session as the screen share / webcam flow sees it
 * (`MediaContext`). Loaded together with `mediaPublish.ts`.
 */

import { t } from "@/i18n";
import { createVoiceAdapter } from "@/lib/webrtc";
import * as tauri from "@/lib/tauri";
import { currentUser } from "@/stores/auth";
import { showToast } from "@/components/ui/Toast";
import { voiceState, setVoiceState, currentChannelSession } from "./voice";
import type { MediaContext } from "./mediaPublish";

export async function voiceMediaContext(): Promise<MediaContext> {
  const viewer = await import("@/stores/screenShareViewer");
  return {
    adapter: () => createVoiceAdapter(),
    session: currentChannelSession,
    connected: () => voiceState.state === "connected",
    channelId: () => voiceState.channelId,
    myUserId: () => currentUser()?.id ?? null,
    myName: () => currentUser()?.display_name || currentUser()?.username || "You",
    sendScreenStart: tauri.wsScreenShareStart,
    sendScreenStop: tauri.wsScreenShareStop,
    sendWebcamStart: tauri.wsWebcamStart,
    sendWebcamStop: tauri.wsWebcamStop,
    setFlags: (flags) => setVoiceState(flags),
    addPreview: (streamId, track, sourceLabel) => {
      const me = currentUser();
      viewer.addAvailableTrack(
        streamId,
        track,
        me?.id ?? "self",
        me?.display_name || me?.username || t("voice.tile.you"),
        sourceLabel,
      );
    },
    removePreview: (streamId) => viewer.removeAvailableTrack(streamId),
    t,
    notify: (text) =>
      showToast({ type: "error", title: text, duration: 8000, id: "media-stopped" }),
    warnStopFailed: () =>
      showToast({
        type: "warning",
        title: t("voice.toast.shareStopped.title"),
        message: t("voice.toast.shareStopped.message"),
        duration: 5000,
        id: "screen-share-stop-warning",
      }),
    newStreamId: () => crypto.randomUUID(),
  };
}
