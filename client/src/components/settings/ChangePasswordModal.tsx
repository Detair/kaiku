import { Component, createSignal, Show } from "solid-js";
import { AlertCircle, CheckCircle2, X } from "lucide-solid";
import { updatePassword, revokeAllOtherSessions } from "@/lib/tauri";
import { createDialog } from "@/lib/a11y/dialog";
import { t } from "@/i18n";
import { errorText } from "@/i18n/errors";

interface ChangePasswordModalProps {
    onClose: () => void;
}

const ChangePasswordModal: Component<ChangePasswordModalProps> = (props) => {
    const dialog = createDialog({ onClose: () => props.onClose() });

    const [currentPassword, setCurrentPassword] = createSignal("");
    const [newPassword, setNewPassword] = createSignal("");
    const [confirmPassword, setConfirmPassword] = createSignal("");
    const [isLoading, setIsLoading] = createSignal(false);
    const [error, setError] = createSignal<string | null>(null);
    const [showRevokePrompt, setShowRevokePrompt] = createSignal(false);
    const [isRevoking, setIsRevoking] = createSignal(false);

    const handleSubmit = async (e: Event) => {
        e.preventDefault();
        if (isLoading()) return;

        if (newPassword() !== confirmPassword()) {
            setError(t("auth.changePassword.mismatch"));
            return;
        }

        if (newPassword().length < 8) {
            setError(t("auth.changePassword.tooShort"));
            return;
        }

        setError(null);
        setIsLoading(true);

        try {
            await updatePassword(currentPassword(), newPassword());
            setShowRevokePrompt(true);
        } catch (err) {
            setError(
                errorText(err, "auth.changePassword.failed", {
                    INVALID_CREDENTIALS: "auth.changePassword.wrongCurrent",
                }),
            );
        } finally {
            setIsLoading(false);
        }
    };

    const handleRevokeAll = async () => {
        setIsRevoking(true);
        try {
            await revokeAllOtherSessions();
        } catch (err) {
            // Best-effort — password already changed successfully
            console.error("Failed to revoke sessions:", err);
        }
        props.onClose();
    };

    return (
        <div
            class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm animate-in fade-in duration-200"
            onClick={props.onClose}
        >
            <div
                {...dialog.dialogProps}
                ref={dialog.ref}
                class="bg-surface-base w-full max-w-md rounded-2xl shadow-xl border border-white/10 flex flex-col overflow-hidden animate-in zoom-in-95 duration-200"
                onClick={(e) => e.stopPropagation()}
            >
                <div class="px-6 py-4 border-b border-white/10 flex items-center justify-between sticky top-0 bg-surface-base z-10">
                    <h2 id={dialog.titleId} class="text-xl font-bold text-text-primary">{t("common.changePassword")}</h2>
                    <button
                        type="button"
                        aria-label={t("common.close")}
                        onClick={props.onClose}
                        class="p-2 -mr-2 text-text-secondary hover:text-text-primary hover:bg-white/5 rounded-xl transition-colors"
                    >
                        <X class="w-5 h-5" />
                    </button>
                </div>

                <Show when={showRevokePrompt()} fallback={
                    <div class="p-6">
                        <Show when={error()}>
                            <div class="mb-6 p-4 bg-status-danger/10 border border-status-danger/20 rounded-xl flex gap-3 text-status-danger">
                                <AlertCircle class="w-5 h-5 shrink-0" />
                                <p class="text-sm">{error()}</p>
                            </div>
                        </Show>

                        <form onSubmit={handleSubmit} class="space-y-4">
                            <div>
                                <label class="block text-sm font-medium text-text-primary mb-2">
                                    {t("auth.changePassword.current")}
                                </label>
                                <input
                                    type="password"
                                    value={currentPassword()}
                                    onInput={(e) => setCurrentPassword(e.currentTarget.value)}
                                    class="w-full px-4 py-2 bg-surface-layer1 border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors disabled:opacity-50"
                                    required
                                    disabled={isLoading()}
                                />
                            </div>

                            <div>
                                <label class="block text-sm font-medium text-text-primary mb-2">
                                    {t("auth.changePassword.new")}
                                </label>
                                <input
                                    type="password"
                                    value={newPassword()}
                                    onInput={(e) => setNewPassword(e.currentTarget.value)}
                                    class="w-full px-4 py-2 bg-surface-layer1 border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors disabled:opacity-50"
                                    required
                                    minLength={8}
                                    disabled={isLoading()}
                                />
                                <p class="text-xs text-text-secondary mt-1">{t("auth.changePassword.newHint")}</p>
                            </div>

                            <div>
                                <label class="block text-sm font-medium text-text-primary mb-2">
                                    {t("auth.changePassword.confirm")}
                                </label>
                                <input
                                    type="password"
                                    value={confirmPassword()}
                                    onInput={(e) => setConfirmPassword(e.currentTarget.value)}
                                    class="w-full px-4 py-2 bg-surface-layer1 border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors disabled:opacity-50"
                                    required
                                    minLength={8}
                                    disabled={isLoading()}
                                />
                            </div>

                            <div class="pt-4 flex justify-end gap-3">
                                <button
                                    type="button"
                                    onClick={props.onClose}
                                    class="px-5 py-2 text-text-primary hover:bg-white/5 rounded-xl font-medium transition-colors"
                                    disabled={isLoading()}
                                >
                                    {t("common.cancel")}
                                </button>
                                <button
                                    type="submit"
                                    disabled={isLoading() || !currentPassword() || !newPassword() || !confirmPassword()}
                                    class="px-5 py-2 bg-accent-primary text-on-accent rounded-xl font-medium hover:bg-accent-primary/90 transition-colors disabled:bg-surface-highlight disabled:text-text-secondary flex items-center justify-center min-w-[120px]"
                                >
                                    {isLoading() ? (
                                        <div class="w-5 h-5 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                                    ) : (
                                        t("common.changePassword")
                                    )}
                                </button>
                            </div>
                        </form>
                    </div>
                }>
                    <div class="p-6 space-y-4">
                        <div class="flex items-center gap-3 p-4 bg-status-success/10 border border-status-success/20 rounded-xl text-text-primary">
                            <CheckCircle2 class="w-5 h-5 shrink-0" />
                            <p class="text-sm font-medium">{t("auth.changePassword.success")}</p>
                        </div>
                        <div class="text-center space-y-2">
                            <p class="text-sm text-text-secondary">
                                {t("auth.changePassword.revokePrompt")}
                            </p>
                        </div>
                        <div class="flex gap-3 justify-end pt-2">
                            <button
                                onClick={props.onClose}
                                class="px-5 py-2 text-text-primary hover:bg-white/5 rounded-xl font-medium transition-colors"
                                disabled={isRevoking()}
                            >
                                {t("auth.changePassword.revokeNo")}
                            </button>
                            <button
                                onClick={handleRevokeAll}
                                disabled={isRevoking()}
                                class="px-5 py-2 bg-status-danger/20 text-status-danger rounded-xl font-medium hover:bg-status-danger/30 transition-colors disabled:opacity-50 flex items-center justify-center min-w-[180px]"
                            >
                                {isRevoking() ? (
                                    <div class="w-5 h-5 border-2 border-current/30 border-t-current rounded-full animate-spin" />
                                ) : (
                                    t("auth.changePassword.revokeYes")
                                )}
                            </button>
                        </div>
                    </div>
                </Show>
            </div>
        </div>
    );
};

export default ChangePasswordModal;
