// Flash r3 p2 #4: the notice texts are a lazy chunk; if it cannot load, the
// OS notification still goes out, with a generic sentence.
localStorage.setItem("kaiku:skip-session-restore", "1");
import { describe, it, expect, vi } from "vitest";

vi.mock("@/components/security/noticeText", () => {
  throw new Error("chunk load failed");
});
const send = vi.hoisted(() => vi.fn(async () => undefined));
vi.mock("@/lib/notifications", async (orig) => ({ ...(await orig<object>()), sendSecurityNotification: send }));

import { handleServerEvent } from "../websocket";
import { notice } from "@/components/security/__tests__/fixtures";
import type { Message } from "@/lib/types";

describe("security notice push without its texts", () => {
  it("still notifies, with a generic sentence", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const n = notice();
    await handleServerEvent({
      type: "message_new",
      channel_id: "k1",
      message: {
        id: "m1",
        channel_id: "k1",
        author: { id: "6b61696b-7500-4000-8000-000000000001", username: "kaiku", display_name: "Kaiku", avatar_url: null, status: "online", is_system: true },
        content: "",
        encrypted: false,
        attachments: [],
        created_at: "2026-11-03T10:00:00Z",
        message_type: "security",
        security_notice: n,
      } as unknown as Message,
    } as never);
    await vi.waitFor(() => expect(send).toHaveBeenCalled());
    expect(send).toHaveBeenCalledWith("Kaiku security notice", "Open Kaiku to see what happened.", n.notice_id);
  });
});
