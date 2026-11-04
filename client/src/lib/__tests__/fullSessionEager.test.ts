// Fable r2 R2: the full-session dialog is a lazy chunk, so its open state
// and event listener must be registered by the app shell itself. A refusal
// reported before any lazy host has loaded still opens the dialog later.
import { describe, it, expect } from "vitest";

describe("full-session refusals before the lazy hosts load", () => {
  it("the app shell registers the listener, so an early refusal is kept", async () => {
    localStorage.setItem("kaiku:skip-session-restore", "1");
    await import("@/App");
    const { reportFullSessionRequired } = await import("@/lib/tauri/common");
    reportFullSessionRequired();
    // Imported only now: if the shell had not loaded it, this is a fresh
    // module whose dialog never heard of the refusal.
    const { fullSessionRequiredOpen } = await import("@/lib/fullSessionRequired");
    expect(fullSessionRequiredOpen()).toBe(true);
    // Importing the whole shell is slow under a cold transform.
  }, 60_000);
});
