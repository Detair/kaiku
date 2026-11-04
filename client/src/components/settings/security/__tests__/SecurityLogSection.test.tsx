import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ listSecurityEvents: vi.fn() }));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));

import SecurityLogSection from "../SecurityLogSection";
import { event } from "@/components/security/__tests__/fixtures";

afterEach(cleanup);

describe("SecurityLogSection", () => {
  it("security log pages with before", async () => {
    api.listSecurityEvents
      .mockResolvedValueOnce({ items: [event({ id: "e3" }), event({ id: "e2", event_type: "password_changed" })], has_more: true, next_cursor: "e2" })
      .mockResolvedValueOnce({ items: [event({ id: "e1", event_type: "recovery_codes_created" })], has_more: false, next_cursor: null });
    render(() => <SecurityLogSection />);
    expect(await screen.findByText("The password of your account was changed.")).toBeTruthy();
    expect(api.listSecurityEvents).toHaveBeenCalledWith(undefined, 50);
    fireEvent.click(screen.getByRole("button", { name: "Show older" }));
    expect(await screen.findByText("New recovery codes were created for your account.")).toBeTruthy();
    expect(api.listSecurityEvents).toHaveBeenLastCalledWith("e2", 50);
    await waitFor(() => expect(screen.queryByRole("button", { name: "Show older" })).toBeNull());
    expect(screen.queryByRole("button", { name: "That wasn't me" })).toBeNull();
  });
});
