import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
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
  isElevationRequiredError: (e: unknown) =>
    (e instanceof Error ? e.message : String(e)).includes("elevation_required"),
  unlinkIdentity: vi.fn(),
}));
vi.mock("@/components/admin/ElevateDialog", () => ({
  default: (p: { onElevated?: () => void; onClose: () => void }) => (
    <button onClick={() => (p.onElevated?.(), p.onClose())}>elevate-now</button>
  ),
}));

import LinkedAccountsSection from "../LinkedAccountsSection";

const needsElevation = () => new Error("elevation_required");
const clickLink = async () =>
  fireEvent.click(await screen.findByRole("button", { name: /Link Keycloak/ }));
const popupStub = () => ({ closed: false, close: vi.fn() }) as unknown as Window;

describe("LinkedAccountsSection admin elevation", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    linkIdentity.mockReset();
  });
  afterEach(() => vi.restoreAllMocks());

  it("does not retry on its own: shows Link now after elevation (desktop)", async () => {
    linkIdentity.mockRejectedValueOnce(needsElevation()).mockResolvedValueOnce({ mode: "tauri" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    fireEvent.click(await screen.findByRole("button", { name: "elevate-now" }));
    const now = await screen.findByRole("button", { name: "Link now" });
    expect(linkIdentity).toHaveBeenCalledTimes(1);
    fireEvent.click(now);
    await waitFor(() => expect(linkIdentity).toHaveBeenCalledTimes(2));
  });

  it("opens the browser popup only from the Link now click", async () => {
    const open = vi.spyOn(window, "open").mockReturnValue(popupStub());
    linkIdentity
      .mockRejectedValueOnce(needsElevation())
      .mockResolvedValueOnce({ mode: "browser", authUrl: "https://idp/x" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    fireEvent.click(await screen.findByRole("button", { name: "elevate-now" }));
    const now = await screen.findByRole("button", { name: "Link now" });
    expect(open).not.toHaveBeenCalled();
    fireEvent.click(now);
    await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
  });

  it("a second elevation_required shows an error instead of asking again", async () => {
    linkIdentity.mockRejectedValue(needsElevation());
    render(() => <LinkedAccountsSection />);
    await clickLink();
    fireEvent.click(await screen.findByRole("button", { name: "elevate-now" }));
    fireEvent.click(await screen.findByRole("button", { name: "Link now" }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.queryByRole("button", { name: "elevate-now" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Link now" })).toBeNull();
  });

  it("a blocked popup shows an error and frees the button", async () => {
    vi.spyOn(window, "open").mockReturnValue(null);
    linkIdentity.mockResolvedValue({ mode: "browser", authUrl: "https://idp/x" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("blocked"));
    expect(
      (screen.getByRole("button", { name: /Link Keycloak/ }) as HTMLButtonElement).disabled,
    ).toBe(false);
  });

  it("a popup that never reports times out and frees the button", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const popup = popupStub();
      vi.spyOn(window, "open").mockReturnValue(popup);
      linkIdentity.mockResolvedValue({ mode: "browser", authUrl: "https://idp/x" });
      render(() => <LinkedAccountsSection />);
      await clickLink();
      await vi.advanceTimersByTimeAsync(121_000);
      await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
      expect((popup as unknown as { close: () => void }).close).toHaveBeenCalled();
      expect(
        (screen.getByRole("button", { name: /Link Keycloak/ }) as HTMLButtonElement).disabled,
      ).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  const callback = (error_code: string) =>
    window.dispatchEvent(
      new MessageEvent("message", {
        origin: "https://chat.example",
        data: { type: "oidc-link-callback", success: false, error_code },
      }),
    );

  it.each([
    ["LINK_SESSION_INVALID", "no longer valid"],
    ["ACCOUNT_BANNED", "suspended"],
    ["SERVICE_UNAVAILABLE", "try again later"],
  ])("maps the link callback error %s to text", async (code, text) => {
    vi.spyOn(window, "open").mockReturnValue(popupStub());
    linkIdentity.mockResolvedValue({ mode: "browser", authUrl: "https://idp/x" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    await waitFor(() => expect(window.open).toHaveBeenCalled());
    callback(code);
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain(text));
  });

  it("elevation_required from the callback starts the elevate, Link now flow", async () => {
    vi.spyOn(window, "open").mockReturnValue(popupStub());
    linkIdentity.mockResolvedValue({ mode: "browser", authUrl: "https://idp/x" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    await waitFor(() => expect(window.open).toHaveBeenCalled());
    callback("elevation_required");
    fireEvent.click(await screen.findByRole("button", { name: "elevate-now" }));
    expect(await screen.findByRole("button", { name: "Link now" })).toBeTruthy();
  });
});
