import { HttpError } from "@/lib/tauri/common";
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

function codeOf(err: unknown): string {
  return err instanceof HttpError ? (err.code ?? "").toLowerCase() : "";
}

/** The call is already gone on the server (404 call_not_found, any case). */
export function isCallAlreadyGoneError(err: unknown): boolean {
  if (codeOf(err) === "call_not_found") return true;
  return err instanceof Error && /call[ _]not[ _]found/i.test(err.message);
}

/** Starting failed because the other side's call exists (409 call_already_exists, glare). */
export function isCallGlareError(err: unknown): boolean {
  return codeOf(err) === "call_already_exists";
}
