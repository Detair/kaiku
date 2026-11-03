import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const tauriMode = vi.hoisted(() => ({ on: false }));
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<object>()), isTauri: () => tauriMode.on }));
const fetchMock = vi.hoisted(() => {
  localStorage.setItem("kaiku:skip-session-restore", "1");
  const f = vi.fn();
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});

const createCredential = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/passkeys", async (orig) => ({
  ...(await orig<object>()),
  createPasskeyCredential: createCredential,
}));

import SetupLink from "../SetupLink";

const TOKEN = "QEFCQ0RFRkdISUpLTE1OT1BRUlNUVVZXWFlaW1xdXl8";
const ID = "55555555-5555-4555-8555-555555555555";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

let ua = "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0";
beforeEach(() => {
  fetchMock.mockReset();
  tauriMode.on = false;
  ua = "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0";
  vi.spyOn(navigator, "userAgent", "get").mockImplementation(() => ua);
  createCredential.mockReset();
  createCredential.mockResolvedValue({ id: "cred", type: "public-key" });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

function open(hash: string) {
  window.history.replaceState(null, "", `/setup${hash}`);
  render(() => (
    <Router>
      <Route path="/setup" component={SetupLink} />
    </Router>
  ));
}

function mountClaim(status: unknown) {
  fetchMock.mockImplementation(async (url: string) => {
    if (url.endsWith("/auth/enrollments/passkey/start")) {
      return json(200, { ceremony_id: "c1", options: { publicKey: {} }, server_name: "Familie Müller", subject: { username: "oma", display_name: "Oma" } });
    }
    if (url.endsWith("/auth/enrollments/passkey/finish")) {
      return json(201, { enrollment_id: ID, state: "claimed", code: "123456", code_expires_at: "" });
    }
    if (url.endsWith("/auth/enrollments/status")) return json(200, status);
    throw new Error(url);
  });
}

describe("SetupLink page", () => {
  it("fragment token is parsed and removed from the address bar", async () => {
    mountClaim({ state: "claimed", authenticator_state: null, ready_at: null });
    open(`#v=1&t=${TOKEN}`);
    await screen.findByText("Set up a passkey for Oma on Familie Müller?");
    expect(window.location.hash).toBe("");
    expect(window.location.href).not.toContain(TOKEN);
  });

  it("unknown version asks to update", async () => {
    open(`#v=9&t=${TOKEN}`);
    expect(await screen.findByText("This link needs a newer Kaiku. Update Kaiku and open the link again.")).toBeTruthy();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("android browser shows Open in Kaiku with the intent url", async () => {
    ua = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 Chrome/130 Mobile Safari/537.36";
    open(`#v=1&t=${TOKEN}`);
    const link = await screen.findByRole("link", { name: "Open in Kaiku" });
    expect(link.getAttribute("href")).toBe(
      `intent://setup/v1?o=${encodeURIComponent(window.location.origin)}&t=${TOKEN}#Intent;scheme=kaiku;end`,
    );
    expect(screen.getByText("Don't have the Kaiku app? Ask the person who sent you this link.")).toBeTruthy();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("desktop app shows the open-in-browser text", async () => {
    tauriMode.on = true;
    open(`#v=1&t=${TOKEN}`);
    expect(await screen.findByText("Open this link in your browser to add a passkey.")).toBeTruthy();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("computer runs the passkey claim and shows the grouped code", async () => {
    mountClaim({ state: "claimed", authenticator_state: null, ready_at: null });
    open(`#v=1&t=${TOKEN}`);
    fireEvent.click(await screen.findByRole("button", { name: "Set up a passkey" }));
    expect(await screen.findByText("123 456")).toBeTruthy();
    expect(screen.getByText("Type this code on the device that showed you the setup code.")).toBeTruthy();
  });

  it("status long-poll shows done, pending ready at and cancelled", async () => {
    for (const [status, text] of [
      [{ state: "done", authenticator_state: "active", ready_at: null }, "Done. Sign in with your passkey."],
      [{ state: "done", authenticator_state: "pending", ready_at: "2026-11-04T09:00:00Z" }, /^Your passkey will be ready at /],
      [{ state: "cancelled", authenticator_state: null, ready_at: null }, "This setup was cancelled."],
    ] as const) {
      mountClaim(status);
      open(`#v=1&t=${TOKEN}`);
      fireEvent.click(await screen.findByRole("button", { name: "Set up a passkey" }));
      expect(await screen.findByText(text)).toBeTruthy();
      cleanup();
    }
  });

  it("an invalid or used link says so", async () => {
    fetchMock.mockImplementation(async () => json(404, { error: "ENROLLMENT_TOKEN_INVALID", message: "x" }));
    open(`#v=1&t=${TOKEN}`);
    expect(await screen.findByText("This setup link is not valid any more. Ask for a new one.")).toBeTruthy();
  });

  it("token is sent only in request bodies", async () => {
    mountClaim({ state: "done", authenticator_state: "active", ready_at: null });
    open(`#v=1&t=${TOKEN}`);
    fireEvent.click(await screen.findByRole("button", { name: "Set up a passkey" }));
    await screen.findByText("Done. Sign in with your passkey.");
    await waitFor(() => expect(fetchMock.mock.calls.length).toBeGreaterThanOrEqual(3));
    for (const [url] of fetchMock.mock.calls as [string][]) expect(url).not.toContain(TOKEN);
  });
});
