import { t } from "@/i18n";

/** User-facing text for a `call_ended` / local end reason. */
export function endReasonText(reason: string): string {
  switch (reason) {
    case "cancelled":
      return t("voice.call.ended.cancelled");
    case "declined":
      return t("voice.call.ended.declined");
    case "all_declined":
      return t("voice.call.ended.allDeclined");
    case "no_answer":
      return t("voice.call.ended.noAnswer");
    case "last_left":
      return t("voice.call.ended.lastLeft");
    default:
      return t("voice.call.ended.default");
  }
}
