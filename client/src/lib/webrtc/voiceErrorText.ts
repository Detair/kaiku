import { t } from "@/i18n";
import type { VoiceError } from "@/lib/webrtc";

/**
 * User-facing text for a VoiceError. Known types get catalog text; adapter
 * messages (English-only, opaque) only appear inside a translated sentence.
 */
export function voiceErrorText(error: VoiceError): string {
  switch (error.type) {
    case "permission_denied":
      return t("voice.error.permissionDenied");
    case "device_not_found":
      return t("voice.error.deviceNotFound");
    case "device_in_use":
      return t("voice.error.deviceInUse");
    case "hardware_error":
    case "constraint_error":
    case "unknown":
      return t("voice.audio.error.device", { message: error.message });
    case "ice_failed":
    case "cancelled":
    case "not_found":
      return t("voice.error.generic", { message: error.message });
    case "server_rejected":
      return t("voice.error.serverRejected", {
        message: error.message,
        code: error.code,
      });
    case "connection_failed":
      return t("voice.error.connectionFailed", { reason: error.reason });
    case "timeout":
      return t("voice.error.timeout", { operation: error.operation });
    case "already_connected":
      return t("voice.error.alreadyConnected", { channelId: error.channelId });
    case "not_connected":
      return t("voice.error.notConnected");
  }
}
