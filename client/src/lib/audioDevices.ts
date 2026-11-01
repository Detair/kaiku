/**
 * Device list filtering for the "Show all ALSA devices" option.
 *
 * On Linux the native adapter returns the curated device list (system
 * default, one entry per card, connected HDMI outputs) followed by every
 * other ALSA PCM with `curated: false`. Those are hidden unless the person
 * turns the option on. Elsewhere every device is curated.
 */

import { createSignal } from "solid-js";
import type { AudioDevice } from "@/lib/webrtc/types";

const STORAGE_KEY = "kaiku.audio.showAllAlsaDevices";

function readStored(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

const [showAllAlsaDevices, setShowAll] = createSignal(readStored());

/** Whether hidden ALSA devices are shown (per device, default off). */
export { showAllAlsaDevices };

export function setShowAllAlsaDevices(show: boolean): void {
  setShowAll(show);
  try {
    localStorage.setItem(STORAGE_KEY, String(show));
  } catch {
    // Storage unavailable: the choice lasts for this session only.
  }
}

/** Whether the list has devices that only the option shows. */
export function hasHiddenDevices(devices: readonly AudioDevice[]): boolean {
  return devices.some((d) => d.curated === false);
}

/**
 * Devices to offer. The selected device always stays listed, so turning
 * the option off never blanks or silently changes the current choice.
 */
export function visibleDevices(
  devices: readonly AudioDevice[],
  showAll: boolean,
  selectedId?: string | null,
): AudioDevice[] {
  if (showAll) return [...devices];
  return devices.filter(
    (d) => d.curated !== false || (selectedId != null && d.deviceId === selectedId),
  );
}

/** The entry that stands for the system default device, if the list has one. */
export function systemDefaultDevice(devices: readonly AudioDevice[]): AudioDevice | undefined {
  return devices.find((d) => d.isDefault);
}

/**
 * Whether the label already names the entry as the system default: the
 * desktop app's own "System default" / "System default (PipeWire)" entry
 * (built by `curate_alsa_pcms` in src-tauri/src/audio/devices.rs). Such an
 * entry gets no extra "(Default)" marker; a real device flagged as default
 * (other desktop platforms, browsers) still does.
 */
export function labelNamesSystemDefault(device: AudioDevice): boolean {
  return device.isDefault && device.curated !== undefined && /^System default\b/.test(device.label);
}

/**
 * Resolve a stored selection to a listed device the way the native backend
 * does (`find_selection` in src-tauri/src/audio/devices.rs): among the
 * curated devices by id, then by the legacy name older versions stored;
 * then the same over every device. The stored value itself is not changed.
 */
export function findSelectedDevice(
  devices: readonly AudioDevice[],
  savedId?: string | null,
): AudioDevice | undefined {
  if (!savedId) return undefined;
  const match = (list: readonly AudioDevice[]) =>
    list.find((d) => d.deviceId === savedId) ??
    list.find((d) => d.legacyName !== undefined && d.legacyName === savedId);
  return match(devices.filter((d) => d.curated !== false)) ?? match(devices);
}

/**
 * Device id to preselect in a picker: the saved device (by id, else by
 * legacy name, see {@link findSelectedDevice}), else the system default
 * entry, else the first device. Empty when the list is empty.
 */
export function preselectedDeviceId(
  devices: readonly AudioDevice[],
  savedId?: string | null,
): string {
  return (
    findSelectedDevice(devices, savedId) ??
    systemDefaultDevice(devices) ??
    devices[0]
  )?.deviceId ?? "";
}
