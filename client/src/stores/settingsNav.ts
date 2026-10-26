/**
 * Settings navigation requests.
 *
 * Lets any component ask for the Settings dialog to open on a given tab
 * (e.g. a voice join failure pointing at Audio settings). UserPanel owns
 * the dialog and consumes the request.
 */
import { createSignal } from "solid-js";

export type SettingsTab =
  | "account"
  | "appearance"
  | "notifications"
  | "focus"
  | "audio"
  | "voice"
  | "privacy"
  | "security";

const [settingsRequest, setSettingsRequest] = createSignal<SettingsTab | null>(null);

export { settingsRequest };

export function requestSettings(tab: SettingsTab): void {
  setSettingsRequest(tab);
}

export function clearSettingsRequest(): void {
  setSettingsRequest(null);
}
