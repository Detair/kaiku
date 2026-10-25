import { describe, it, expect, vi } from "vitest";
import { createSignal, Show, type Component } from "solid-js";
import { render, screen, fireEvent } from "@solidjs/testing-library";

vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<object>()),
  invoke: vi.fn(),
}));
vi.mock("@/lib/tauri", () => ({
  initE2EE: vi.fn(),
  mfaSetup: vi.fn(() => new Promise(() => {})),
  mfaVerify: vi.fn(),
  updatePassword: vi.fn(),
  revokeAllOtherSessions: vi.fn(),
}));
vi.mock("@/lib/featureFlags", () => ({ isE2EEEnabled: () => false }));
vi.mock("qrcode", () => ({ default: { toDataURL: vi.fn() } }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/lib/clipboard", () => ({ secureCopy: vi.fn() }));
vi.mock("../settings/BackupCodesDisplay", () => ({ default: () => null }));
vi.mock("../settings/AccountSettings", () => ({ default: () => null }));
vi.mock("../settings/AppearanceSettings", () => ({ default: () => null }));
vi.mock("../settings/NotificationSettings", () => ({ default: () => null }));
vi.mock("../settings/SecuritySettings", () => ({ default: () => null }));
vi.mock("../settings/PrivacySettings", () => ({ default: () => null }));
vi.mock("../settings/FocusSettings", () => ({ default: () => null }));
vi.mock("../settings/AudioSettings", () => ({ default: () => null }));
vi.mock("../settings/VoiceSettings", () => ({ default: () => null }));

import SettingsModal from "../settings/SettingsModal";
import RecoveryKeyModal from "../settings/RecoveryKeyModal";
import ChangePasswordModal from "../settings/ChangePasswordModal";
import MfaSetupModal from "../settings/MfaSetupModal";

const children: Array<[string, Component<{ onClose: () => void }>]> = [
  [
    "RecoveryKeyModal",
    (p) => <RecoveryKeyModal keyChunks={["a"]} fullKey="a" onConfirm={() => {}} onClose={p.onClose} />,
  ],
  ["ChangePasswordModal", (p) => <ChangePasswordModal onClose={p.onClose} />],
  ["MfaSetupModal", (p) => <MfaSetupModal onClose={p.onClose} onComplete={() => {}} />],
];

describe("Settings child modals sit on top of Settings", () => {
  for (const [name, Child] of children) {
    it(`Escape in ${name} closes only the child; Tab stays inside it`, async () => {
      const settingsClose = vi.fn();
      const childClose = vi.fn();
      const [open, setOpen] = createSignal(false);
      render(() => (
        <>
          <SettingsModal onClose={settingsClose} />
          <Show when={open()}>
            <Child onClose={childClose} />
          </Show>
        </>
      ));
      setOpen(true);
      await Promise.resolve();
      const dialogs = screen.getAllByRole("dialog");
      expect(dialogs.length).toBe(2);
      const settings = screen.getByRole("dialog", { name: "Settings" });
      const child = dialogs.find((d) => d !== settings)!;

      const items = Array.from(
        child.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled])"),
      );
      expect(items.length).toBeGreaterThan(0);
      const last = items[items.length - 1];
      last.focus();
      fireEvent.keyDown(last, { key: "Tab" });
      expect(child.contains(document.activeElement)).toBe(true);
      expect(settings.contains(document.activeElement)).toBe(false);

      fireEvent.keyDown(document.body, { key: "Escape" });
      expect(childClose).toHaveBeenCalledTimes(1);
      expect(settingsClose).not.toHaveBeenCalled();
    });
  }
});
