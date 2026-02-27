import { Component, createSignal, Show } from "solid-js";
import { AlertCircle, CheckCircle2, X } from "lucide-solid";
import { updatePassword } from "@/lib/tauri";

interface ChangePasswordModalProps {
    onClose: () => void;
}

const ChangePasswordModal: Component<ChangePasswordModalProps> = (props) => {
    const [currentPassword, setCurrentPassword] = createSignal("");
    const [newPassword, setNewPassword] = createSignal("");
    const [confirmPassword, setConfirmPassword] = createSignal("");

    const [isLoading, setIsLoading] = createSignal(false);
    const [error, setError] = createSignal<string | null>(null);
    const [success, setSuccess] = createSignal(false);

    const handleSubmit = async (e: Event) => {
        e.preventDefault();
        if (isLoading()) return;

        if (newPassword() !== confirmPassword()) {
            setError("New passwords do not match");
            return;
        }

        if (newPassword().length < 8) {
            setError("New password must be at least 8 characters long");
            return;
        }

        setError(null);
        setIsLoading(true);

        try {
            await updatePassword(currentPassword(), newPassword());
            setSuccess(true);
            setTimeout(() => {
                props.onClose();
            }, 2000);
        } catch (err) {
            setError(err instanceof Error ? err.message : "Failed to update password");
        } finally {
            setIsLoading(false);
        }
    };

    return (
        <div
            class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm animate-in fade-in duration-200"
            onClick={props.onClose}
        >
            <div
                class="bg-surface-base w-full max-w-md rounded-2xl shadow-xl border border-white/10 flex flex-col overflow-hidden animate-in zoom-in-95 duration-200"
                onClick={(e) => e.stopPropagation()}
            >
                <div class="px-6 py-4 border-b border-white/10 flex items-center justify-between sticky top-0 bg-surface-base z-10">
                    <h2 class="text-xl font-bold text-text-primary">Change Password</h2>
                    <button
                        onClick={props.onClose}
                        class="p-2 -mr-2 text-text-secondary hover:text-text-primary hover:bg-white/5 rounded-xl transition-colors"
                    >
                        <X class="w-5 h-5" />
                    </button>
                </div>

                <div class="p-6">
                    <Show when={error()}>
                        <div class="mb-6 p-4 bg-status-danger/10 border border-status-danger/20 rounded-xl flex gap-3 text-status-danger">
                            <AlertCircle class="w-5 h-5 shrink-0" />
                            <p class="text-sm">{error()}</p>
                        </div>
                    </Show>

                    <Show when={success()}>
                        <div class="mb-6 p-4 bg-status-success/10 border border-status-success/20 rounded-xl flex items-center gap-3 text-status-success">
                            <CheckCircle2 class="w-5 h-5 shrink-0" />
                            <p class="text-sm font-medium">Password updated successfully!</p>
                        </div>
                    </Show>

                    <form onSubmit={handleSubmit} class="space-y-4">
                        <div>
                            <label class="block text-sm font-medium text-text-primary mb-2">
                                Current Password
                            </label>
                            <input
                                type="password"
                                value={currentPassword()}
                                onInput={(e) => setCurrentPassword(e.currentTarget.value)}
                                class="w-full px-4 py-2 bg-surface-layer1 border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors disabled:opacity-50"
                                required
                                disabled={isLoading() || success()}
                            />
                        </div>

                        <div>
                            <label class="block text-sm font-medium text-text-primary mb-2">
                                New Password
                            </label>
                            <input
                                type="password"
                                value={newPassword()}
                                onInput={(e) => setNewPassword(e.currentTarget.value)}
                                class="w-full px-4 py-2 bg-surface-layer1 border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors disabled:opacity-50"
                                required
                                minLength={8}
                                disabled={isLoading() || success()}
                            />
                            <p class="text-xs text-text-secondary mt-1">Must be at least 8 characters long.</p>
                        </div>

                        <div>
                            <label class="block text-sm font-medium text-text-primary mb-2">
                                Confirm New Password
                            </label>
                            <input
                                type="password"
                                value={confirmPassword()}
                                onInput={(e) => setConfirmPassword(e.currentTarget.value)}
                                class="w-full px-4 py-2 bg-surface-layer1 border border-white/10 rounded-xl text-text-primary focus:outline-none focus:border-accent-primary focus:ring-1 focus:ring-accent-primary transition-colors disabled:opacity-50"
                                required
                                minLength={8}
                                disabled={isLoading() || success()}
                            />
                        </div>

                        <div class="pt-4 flex justify-end gap-3">
                            <button
                                type="button"
                                onClick={props.onClose}
                                class="px-5 py-2 text-text-primary hover:bg-white/5 rounded-xl font-medium transition-colors"
                                disabled={isLoading() || success()}
                            >
                                Cancel
                            </button>
                            <button
                                type="submit"
                                disabled={isLoading() || success() || !currentPassword() || !newPassword() || !confirmPassword()}
                                class="px-5 py-2 bg-accent-primary text-white rounded-xl font-medium hover:bg-accent-primary/90 transition-colors disabled:bg-surface-highlight disabled:text-text-secondary flex items-center justify-center min-w-[120px]"
                            >
                                {isLoading() ? (
                                    <div class="w-5 h-5 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                                ) : success() ? (
                                    "Updated"
                                ) : (
                                    "Change Password"
                                )}
                            </button>
                        </div>
                    </form>
                </div>
            </div>
        </div>
    );
};

export default ChangePasswordModal;
