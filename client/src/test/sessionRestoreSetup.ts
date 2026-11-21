/**
 * Vitest setup: no browser session restore at import.
 *
 * `lib/tauri/common.ts` restores the session when it is first imported: a
 * real refresh request that nobody awaits. In a test it can outlive the test
 * file and log after the environment is torn down, which fails the whole run
 * with an unhandled error. The module skips the restore while
 * `kaiku:skip-session-restore` is set, so the flag is set here for every test
 * file and kept through `localStorage.clear()`.
 *
 * A test about the restore itself opts in by removing the flag (after any
 * `localStorage.clear()`) before it imports the module.
 */

export const SKIP_SESSION_RESTORE_KEY = "kaiku:skip-session-restore";

// Test files in the `node` environment have no storage (and no session).
if (typeof localStorage !== "undefined") {
  localStorage.setItem(SKIP_SESSION_RESTORE_KEY, "1");

  const clearStorage = Storage.prototype.clear;
  Storage.prototype.clear = function clear(this: Storage): void {
    clearStorage.call(this);
    if (this === globalThis.localStorage) this.setItem(SKIP_SESSION_RESTORE_KEY, "1");
  };
}
