import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";

const linkIdentity = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri", () => ({
  listIdentities: vi.fn(async () => ({ identities: [] })),
  fetchServerSettings: vi.fn(async () => ({
    oidc_enabled: true,
    oidc_providers: [{ slug: "kc", display_name: "Keycloak", icon_hint: null }],
  })),
  getServerUrl: () => "https://chat.example",
  linkIdentity: (...a: unknown[]) => linkIdentity(...a),
  isElevationRequiredError: (e: unknown) => e instanceof Error && e.message === "elevation_required",
  unlinkIdentity: vi.fn(),
}));
vi.mock("@/components/admin/ElevateDialog", () => ({
  default: (p: { onElevated?: () => void; onClose: () => void }) => (
    <button onClick={() => (p.onElevated?.(), p.onClose())}>elevate-now</button>
  ),
}));

import LinkedAccountsSection from "../LinkedAccountsSection";

describe("LinkedAccountsSection", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    linkIdentity.mockReset();
  });

  it("asks an admin to re-authenticate, then retries the link", async () => {
    linkIdentity
      .mockRejectedValueOnce(new Error("elevation_required"))
      .mockResolvedValueOnce({ mode: "tauri" });
    render(() => <LinkedAccountsSection />);
    fireEvent.click(await screen.findByRole("button", { name: /Link Keycloak/ }));
    fireEvent.click(await screen.findByRole("button", { name: "elevate-now" }));
    await waitFor(() => expect(linkIdentity).toHaveBeenCalledTimes(2));
  });
});
