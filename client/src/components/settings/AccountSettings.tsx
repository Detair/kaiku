import { Component, createSignal, Show } from "solid-js";
import { Camera, Upload } from "lucide-solid";
import { authState, updateUser } from "@/stores/auth";
import Avatar from "@/components/ui/Avatar";
import * as tauri from "@/lib/tauri";
import { validateFileSize, getUploadLimitText } from "@/lib/tauri";
import { formatFileSize } from "@/lib/tauri/common";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";
import ChangePasswordModal from "./ChangePasswordModal";
import LinkedAccountsSection from "./LinkedAccountsSection";
import SessionsSection from "./SessionsSection";

const AccountSettings: Component = () => {
  const user = () => authState.user;
  const [isUploading, setIsUploading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [isPasswordModalOpen, setIsPasswordModalOpen] = createSignal(false);
  let fileInput: HTMLInputElement | undefined;

  const handleFileChange = async (e: Event) => {
    const target = e.target as HTMLInputElement;
    const file = target.files?.[0];
    if (!file) return;

    setError(null);

    // Frontend validation before attempting upload
    const validationError = validateFileSize(file, "avatar");
    if (validationError) {
      setError(
        t("settings.account.avatarTooLarge", {
          size: formatFileSize(file.size),
          max: getUploadLimitText("avatar"),
        }),
      );
      return;
    }

    // Validate file type
    if (!file.type.startsWith("image/")) {
      setError(t("settings.account.avatarNotImage"));
      return;
    }

    setIsUploading(true);

    try {
      const updatedUser = await tauri.uploadAvatar(file);
      updateUser(updatedUser);
      showToast({
        type: "success",
        title: t("settings.account.avatarUpdatedTitle"),
        message: t("settings.account.avatarUpdatedMessage"),
        duration: 3000,
      });
    } catch (err) {
      console.error("Failed to upload avatar:", err);
      const errorMsg = errorText(err, "settings.account.uploadFailed");
      setError(errorMsg);
      showToast({
        type: "error",
        title: t("settings.account.uploadFailedTitle"),
        message: errorMsg,
        duration: 8000,
      });
    } finally {
      setIsUploading(false);
      // Reset input
      if (fileInput) fileInput.value = "";
    }
  };

  return (
    <div class="space-y-6">
      <div>
        <h3 class="text-lg font-semibold text-text-primary mb-1">{t("settings.account.title")}</h3>
        <p class="text-sm text-text-secondary">
          {t("settings.account.subtitle")}
        </p>
      </div>

      {/* Profile Header */}
      <div class="flex items-start gap-6 p-4 rounded-xl bg-surface-base border border-white/5">
        {/* Avatar Section */}
        <div class="relative group">
          <Avatar
            src={user()?.avatar_url}
            alt={user()?.display_name || "?"}
            size="lg"
            status={user()?.status}
            showStatus
          />
          <div
            class="absolute inset-0 bg-black/50 rounded-full flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer"
            onClick={() => fileInput?.click()}
          >
            <Camera class="w-6 h-6 text-white" />
          </div>
          <input
            ref={fileInput}
            type="file"
            accept="image/*"
            class="hidden"
            onChange={handleFileChange}
          />
        </div>

        {/* User Info */}
        <div class="flex-1 min-w-0">
          <div class="flex items-center justify-between mb-2">
            <h4 class="text-xl font-bold text-text-primary truncate">
              {user()?.display_name}
            </h4>
            <button
              class="btn-primary py-1.5 px-3 text-sm flex items-center gap-2"
              onClick={() => fileInput?.click()}
              disabled={isUploading()}
            >
              <Upload class="w-4 h-4" />
              {isUploading() ? t("settings.account.uploading") : t("settings.account.changeAvatar")}
            </button>
          </div>
          <p class="text-xs text-text-secondary">
            {t("settings.account.maxSize", { size: getUploadLimitText("avatar") })}
          </p>
          <div class="space-y-1">
            <div class="flex items-center gap-2 text-sm">
              <span class="text-text-secondary w-20">{t("settings.account.username")}</span>
              <span class="text-text-primary font-mono">
                @{user()?.username}
              </span>
            </div>
            <div class="flex items-center gap-2 text-sm">
              <span class="text-text-secondary w-20">{t("settings.account.email")}</span>
              <span class="text-text-primary">
                {user()?.email || t("settings.account.emailNotSet")}
              </span>
            </div>
            <div class="flex items-center gap-2 text-sm">
              <span class="text-text-secondary w-20">{t("settings.account.userId")}</span>
              <span class="text-text-secondary text-xs font-mono select-all">
                {user()?.id}
              </span>
            </div>
          </div>
        </div>
      </div>

      <Show when={error()}>
        <div class="p-3 rounded-lg bg-error-bg border border-error-border text-error-text text-sm">
          {error()}
        </div>
      </Show>

      <div class="pt-4 border-t border-white/5">
        <h4 class="text-sm font-semibold text-text-secondary uppercase tracking-wide mb-4">
          {t("settings.account.passwordSection")}
        </h4>
        <button
          class="w-full text-left px-4 py-3 rounded-xl bg-surface-layer2 hover:bg-surface-highlight border border-white/5 transition-colors"
          onClick={() => setIsPasswordModalOpen(true)}
        >
          <div class="font-medium text-text-primary">{t("common.changePassword")}</div>
          <div class="text-xs text-text-secondary mt-0.5">
            {t("settings.account.changePasswordHint")}
          </div>
        </button>
      </div>

      {/* Linked accounts */}
      <div class="pt-4 border-t border-white/5">
        <LinkedAccountsSection />
      </div>

      {/* Sessions */}
      <div class="pt-4 border-t border-white/5">
        <SessionsSection />
      </div>

      <Show when={isPasswordModalOpen()}>
        <ChangePasswordModal onClose={() => setIsPasswordModalOpen(false)} />
      </Show>
    </div>
  );
};

export default AccountSettings;
