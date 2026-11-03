// Module-level session restore would hit fetch on import.
localStorage.setItem("kaiku:skip-session-restore", "1");
import { describe, it, expect, vi } from "vitest";

const security = vi.hoisted(() => ({ refreshMeSecurity: vi.fn() }));
vi.mock("@/stores/security", async (orig) => ({ ...(await orig<object>()), ...security }));

import { handleServerEvent } from "../websocket";
import { ENROLLMENT_UPDATED_EVENT } from "@/lib/types/events";

describe("security hints over the WebSocket", () => {
  it("authenticators_changed and account_request_updated refetch the overview", async () => {
    await handleServerEvent({ type: "authenticators_changed", user_id: "u1" });
    await handleServerEvent({ type: "account_request_updated", request_id: "r1", subject_id: "u1", state: "ready" });
    expect(security.refreshMeSecurity).toHaveBeenCalledTimes(2);
  });

  it("enrollment_updated is passed on as a window event (refetch, never acted on alone)", async () => {
    const seen = vi.fn();
    window.addEventListener(ENROLLMENT_UPDATED_EVENT, seen);
    await handleServerEvent({ type: "enrollment_updated", enrollment_id: "e1", state: "claimed" });
    expect((seen.mock.calls[0][0] as CustomEvent).detail).toEqual({ enrollment_id: "e1" });
    window.removeEventListener(ENROLLMENT_UPDATED_EVENT, seen);
  });
});
