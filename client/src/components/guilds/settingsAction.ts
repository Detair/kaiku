/**
 * Server-settings writes report what happened (ADMIN-6): a failure shows a
 * toast with what did not work and what to do, a success can show one too.
 * Never just `console.error`.
 */
import { showToast } from "@/components/ui/Toast";
import { actionErrorText } from "@/components/admin/actionErrors";

/** Show why `title` did not work. */
export function reportFailure(title: string, err: unknown): void {
  console.error("[guild settings]", title, err);
  showToast({ type: "error", title, message: actionErrorText(err), duration: 8000 });
}

/** Show that `title` worked. */
export function reportSuccess(title: string, message?: string): void {
  showToast({ type: "success", title, message, duration: 3000 });
}

/**
 * Run `fn`; on failure show `failureTitle` with the reason and return false.
 * On success show `successTitle` when given and return true.
 */
export async function runSettingsAction(
  fn: () => Promise<unknown>,
  failureTitle: string,
  successTitle?: string,
): Promise<boolean> {
  try {
    await fn();
  } catch (err) {
    reportFailure(failureTitle, err);
    return false;
  }
  if (successTitle) reportSuccess(successTitle);
  return true;
}
