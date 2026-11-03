import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";

const linkIdentity = vi.hoisted(() => vi.fn());
const native = vi.hoisted(() => ({ on: false }));
vi.mock("@/lib/tauri", () => ({
  listIdentities: vi.fn(async () => ({ identities: [] })),
  fetchServerSettings: vi.fn(async () => ({
    oidc_enabled: true,
    oidc_providers: [{ slug: "kc", display_name: "Keycloak", icon_hint: null }],
  })),
  getServerUrl: () => "https://chat.example",
  isTauri: () => native.on,
  linkIdentity: (...a: unknown[]) => linkIdentity(...a),
  isElevationRequiredError: (e: unknown) =>
    (e instanceof Error ? e.message : String(e)).toLowerCase().includes("elevation_required"),
  unlinkIdentity: vi.fn(),
}));
const stepUps = vi.hoisted(() => ({ seen: [] as unknown[], cancel: false }));
vi.mock("@/lib/stepUp", async (orig) => {
  const actual = await orig<typeof import("@/lib/stepUp")>();
  return {
    ...actual,
    withStepUp: async (req: unknown, run: () => Promise<unknown>) => {
      stepUps.seen.push(req);
      if (stepUps.cancel) throw new actual.StepUpCancelled();
      return run();
    },
  };
});
vi.mock("@/components/admin/ElevateDialog", () => ({
  default: (p: { onElevated?: () => void; onClose: () => void }) => (
    <button onClick={() => (p.onElevated?.(), p.onClose())}>elevate-now</button>
  ),
}));

import LinkedAccountsSection from "../LinkedAccountsSection";
import { HttpError } from "@/lib/tauri/common";

const needsElevation = () => new Error("elevation_required");
// "Link Keycloak" asks for the `identity.link` step-up; the link starts from "Link now".
const clickLink = async () => {
  fireEvent.click(await screen.findByRole("button", { name: /Link Keycloak/ }));
  fireEvent.click(await screen.findByRole("button", { name: "Link now" }));
};
type FakePopup = { closed: boolean; close: ReturnType<typeof vi.fn>; location: { href: string } };
const fakePopup = (): FakePopup => ({ closed: false, close: vi.fn(), location: { href: "" } });
const popupStub = () => fakePopup() as unknown as Window;

describe("LinkedAccountsSection admin elevation", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    linkIdentity.mockReset();
    native.on = false;
    stepUps.seen.length = 0;
    stepUps.cancel = false;
  });
  afterEach(() => vi.restoreAllMocks());

  it("does not retry on its own: shows Link now after elevation (desktop)", async () => {
    native.on = true;
    linkIdentity.mockRejectedValueOnce(needsElevation()).mockResolvedValueOnce({ mode: "tauri" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    fireEvent.click(await screen.findByRole("button", { name: "elevate-now" }));
    const now = await screen.findByRole("button", { name: "Link now" });
    expect(linkIdentity).toHaveBeenCalledTimes(1);
    fireEvent.click(now);
    await waitFor(() => expect(linkIdentity).toHaveBeenCalledTimes(2));
  });

  it("opens the provider popup only from the Link now click", async () => {
    const first = fakePopup();
    const second = fakePopup();
    const open = vi
      .spyOn(window, "open")
      .mockReturnValueOnce(first as unknown as Window)
      .mockReturnValueOnce(second as unknown as Window);
    linkIdentity
      .mockRejectedValueOnce(needsElevation())
      .mockResolvedValueOnce({ mode: "browser", authUrl: "https://idp/x" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    fireEvent.click(await screen.findByRole("button", { name: "elevate-now" }));
    const now = await screen.findByRole("button", { name: "Link now" });
    // The first click's placeholder was closed when elevation was required.
    expect(first.close).toHaveBeenCalled();
    expect(first.location.href).toBe("");
    fireEvent.click(now);
    expect(open).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(second.location.href).toBe("https://idp/x"));
  });

  it("a second elevation_required shows an error instead of asking again", async () => {
    vi.spyOn(window, "open").mockImplementation(() => popupStub());
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
    // Blocked before anything was asked of the server.
    expect(linkIdentity).not.toHaveBeenCalled();
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

  // The callback page posts from the popup the component opened.
  const callback = (error_code: string, source: unknown = lastPopup()) =>
    window.dispatchEvent(
      new MessageEvent("message", {
        origin: "https://chat.example",
        source: source as MessageEventSource,
        data: { type: "oidc-link-callback", success: false, error_code },
      }),
    );
  const lastPopup = () => {
    const results = vi.mocked(window.open).mock.results;
    return results[results.length - 1]?.value as FakePopup | undefined;
  };

  it("opens the popup inside the click, before the authorize request resolves", async () => {
    const popup = fakePopup();
    vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
    let resolve!: (v: unknown) => void;
    linkIdentity.mockReturnValue(new Promise((r) => (resolve = r)));
    render(() => <LinkedAccountsSection />);
    await clickLink();
    // Synchronously within the click: the request has not answered yet.
    expect(window.open).toHaveBeenCalledTimes(1);
    resolve({ mode: "browser", authUrl: "https://idp/x" });
    await waitFor(() => expect(popup.location.href).toBe("https://idp/x"));
  });

  it("closes the placeholder popup when the authorize request fails", async () => {
    const popup = fakePopup();
    vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
    linkIdentity.mockRejectedValue(new Error("boom"));
    render(() => <LinkedAccountsSection />);
    await clickLink();
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(popup.close).toHaveBeenCalled();
  });

  it("closes the placeholder popup when elevation is required first", async () => {
    const popup = fakePopup();
    vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
    linkIdentity.mockRejectedValue(needsElevation());
    render(() => <LinkedAccountsSection />);
    await clickLink();
    await screen.findByRole("button", { name: "elevate-now" });
    expect(popup.close).toHaveBeenCalled();
  });

  it("after dispose the old attempt's timers never touch a new attempt's popup", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const first = fakePopup();
      const second = fakePopup();
      vi.spyOn(window, "open")
        .mockReturnValueOnce(first as unknown as Window)
        .mockReturnValueOnce(second as unknown as Window);
      linkIdentity.mockResolvedValue({ mode: "browser", authUrl: "https://idp/x" });
      const view = render(() => <LinkedAccountsSection />);
      await clickLink();
      await waitFor(() => expect(first.location.href).toBe("https://idp/x"));
      view.unmount();
      render(() => <LinkedAccountsSection />);
      await clickLink();
      await waitFor(() => expect(second.location.href).toBe("https://idp/x"));
      // Each attempt gets its own window name, so the browser never reuses one.
      const names = vi.mocked(window.open).mock.calls.map((c) => c[1]);
      expect(names[0]).not.toBe(names[1]);
      // The first component's ceiling would fire here; it must be gone.
      await vi.advanceTimersByTimeAsync(60_000);
      first.closed = true; // a stale poll would now report "closed"
      await vi.advanceTimersByTimeAsync(61_000);
      expect(first.close).not.toHaveBeenCalled();
      // The live attempt still times out on its own schedule.
      expect(second.close).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("dispose while authorize is pending: no navigation, no listener, no timers", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const popup = fakePopup();
      vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
      const addListener = vi.spyOn(window, "addEventListener");
      let resolve!: (v: unknown) => void;
      linkIdentity.mockReturnValue(new Promise((r) => (resolve = r)));
      const view = render(() => <LinkedAccountsSection />);
      await clickLink();
      expect(window.open).toHaveBeenCalledTimes(1);
      view.unmount();
      expect(popup.close).toHaveBeenCalledTimes(1);
      resolve({ mode: "browser", authUrl: "https://idp/x" });
      await vi.advanceTimersByTimeAsync(0);
      expect(popup.location.href).toBe("");
      expect(addListener.mock.calls.filter((c) => c[0] === "message")).toHaveLength(0);
      expect(vi.getTimerCount()).toBe(0);
      await vi.advanceTimersByTimeAsync(121_000);
      expect(popup.close).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("ignores a callback message from another window", async () => {
    vi.spyOn(window, "open").mockReturnValue(popupStub());
    linkIdentity.mockResolvedValue({ mode: "browser", authUrl: "https://idp/x" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    await waitFor(() => expect(lastPopup()?.location.href).toBe("https://idp/x"));
    callback("ACCOUNT_BANNED", fakePopup());
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByRole("alert")).toBeNull();
    callback("ACCOUNT_BANNED");
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("suspended"));
  });

  it.each([
    ["LINK_SESSION_INVALID", "no longer valid"],
    ["ACCOUNT_BANNED", "suspended"],
    ["SERVICE_UNAVAILABLE", "try again later"],
    ["link_session_invalid", "no longer valid"],
    ["account_banned", "suspended"],
    ["service_unavailable", "try again later"],
  ])("maps the link callback error %s to text", async (code, text) => {
    vi.spyOn(window, "open").mockReturnValue(popupStub());
    linkIdentity.mockResolvedValue({ mode: "browser", authUrl: "https://idp/x" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    await waitFor(() => expect(window.open).toHaveBeenCalled());
    callback(code);
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain(text));
  });

  it.each(["elevation_required", "ELEVATION_REQUIRED"])("%s from the callback starts the elevate, Link now flow", async (code) => {
    vi.spyOn(window, "open").mockReturnValue(popupStub());
    linkIdentity.mockResolvedValue({ mode: "browser", authUrl: "https://idp/x" });
    render(() => <LinkedAccountsSection />);
    await clickLink();
    await waitFor(() => expect(window.open).toHaveBeenCalled());
    callback(code);
    expect(lastPopup()?.close).toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("button", { name: "elevate-now" }));
    expect(await screen.findByRole("button", { name: "Link now" })).toBeTruthy();
  });

  it("link asks for the identity.link step-up before anything starts", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => popupStub());
    linkIdentity.mockResolvedValue({ mode: "browser", authUrl: "https://idp/x" });
    render(() => <LinkedAccountsSection />);
    fireEvent.click(await screen.findByRole("button", { name: /Link Keycloak/ }));
    await screen.findByRole("button", { name: "Link now" });
    expect(stepUps.seen).toEqual([{ action: "identity.link", target: "", targetLabel: "Keycloak" }]);
    expect(open).not.toHaveBeenCalled();
    expect(linkIdentity).not.toHaveBeenCalled();
  });

  it("a cancelled step-up starts no link and shows no error", async () => {
    stepUps.cancel = true;
    render(() => <LinkedAccountsSection />);
    fireEvent.click(await screen.findByRole("button", { name: /Link Keycloak/ }));
    await waitFor(() => expect(stepUps.seen).toHaveLength(1));
    expect(screen.queryByRole("button", { name: "Link now" })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("an expired link grant asks to confirm again", async () => {
    native.on = true;
    linkIdentity.mockRejectedValue(new HttpError(403, "STEP_UP_REQUIRED", "STEP_UP_REQUIRED"));
    render(() => <LinkedAccountsSection />);
    await clickLink();
    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Please confirm this with a passkey first."));
  });

  it("unlink runs under the identity.unlink grant for that identity", async () => {
    const tauri = await import("@/lib/tauri");
    vi.mocked(tauri.listIdentities).mockResolvedValueOnce({
      identities: [
        {
          id: "0192f0c0-0000-7000-8000-0000000000aa",
          provider_slug: "kc",
          provider_name: "Keycloak",
          email: null,
          created_at: "2026-11-01T00:00:00Z",
          last_used_at: null,
        },
      ],
    } as never);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(() => <LinkedAccountsSection />);
    fireEvent.click(await screen.findByTitle("Unlink Keycloak"));
    await waitFor(() => expect(tauri.unlinkIdentity).toHaveBeenCalledWith("0192f0c0-0000-7000-8000-0000000000aa"));
    expect(stepUps.seen).toEqual([
      { action: "identity.unlink", target: "0192f0c0-0000-7000-8000-0000000000aa", targetLabel: "Keycloak" },
    ]);
  });
});
