// Codex review M3: a fresh desktop start has no session to name a server;
// recovery uses the sign-in page's server (or the native override), never
// the app's own origin.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const tauriMode = vi.hoisted(() => ({ on: true }));
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<object>()), isTauri: () => tauriMode.on, invoke: vi.fn() }));
const api = vi.hoisted(() => ({ startAccountRequest: vi.fn(), requestOidcAuthorize: vi.fn() }));
vi.mock("@/lib/tauri/accountRequests", async (orig) => ({ ...(await orig<object>()), ...api }));
const settings = vi.hoisted(() => vi.fn().mockResolvedValue(null));
vi.mock("@/lib/tauri/auth", async (orig) => ({ ...(await orig<object>()), fetchServerSettings: settings }));
const override = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/settings", async (orig) => ({ ...(await orig<object>()), getDefaultServerUrl: override }));

import RecoverAccount from "../RecoverAccount";
import { browserState } from "@/lib/tauri/common";
import { forgetSignInServer, setSignInServer } from "@/lib/signInServer";
import { requestOf, routed } from "./accountRequestTestUtils";

beforeEach(() => {
  settings.mockClear();
  api.startAccountRequest.mockReset();
  override.mockReset();
  // A fresh desktop start: no session, the webview's own origin.
  browserState.serverUrl = "tauri://localhost";
  sessionStorage.clear();
  forgetSignInServer();
  window.history.pushState({}, "", "/recover");
});
afterEach(cleanup);

describe("desktop recovery without a session", () => {
  it("asks the native override's server and starts the request there", async () => {
    override.mockResolvedValue("https://chat.example.com");
    api.startAccountRequest.mockResolvedValue(requestOf());
    render(routed(RecoverAccount, "/recover"));
    await waitFor(() => expect(settings).toHaveBeenCalledWith("https://chat.example.com"));
    fireEvent.input(screen.getByLabelText("Username"), { target: { value: "alice" } });
    fireEvent.input(screen.getByLabelText("Recovery code"), { target: { value: "abcde fghjk mnp" } });
    fireEvent.input(screen.getByLabelText("Password"), { target: { value: "pw-123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Start recovery" }));
    await waitFor(() => expect(api.startAccountRequest).toHaveBeenCalled());
    expect(api.startAccountRequest.mock.calls[0][0]).toBe("https://chat.example.com");
  });

  it("nothing goes out before the server is known (Flash r3 p3 #4)", async () => {
    let answer!: (url: string | null) => void;
    override.mockReturnValue(new Promise((r) => (answer = r)));
    api.startAccountRequest.mockResolvedValue(requestOf());
    render(routed(RecoverAccount, "/recover"));
    fireEvent.input(screen.getByLabelText("Username"), { target: { value: "alice" } });
    fireEvent.input(screen.getByLabelText("Recovery code"), { target: { value: "abcde fghjk mnp" } });
    fireEvent.input(screen.getByLabelText("Password"), { target: { value: "pw-123456" } });
    const start = screen.getByRole("button", { name: "Start recovery" }) as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    fireEvent.submit(start.closest("form")!);
    await new Promise((r) => setTimeout(r, 10));
    expect(api.startAccountRequest).not.toHaveBeenCalled();
    answer("https://chat.example.com");
    await waitFor(() => expect(start.disabled).toBe(false));
    fireEvent.click(start);
    await waitFor(() => expect(api.startAccountRequest).toHaveBeenCalled());
    expect(api.startAccountRequest.mock.calls[0][0]).toBe("https://chat.example.com");
  });

  it("without any server the form sends nothing", async () => {
    override.mockResolvedValue(null);
    render(routed(RecoverAccount, "/recover"));
    await waitFor(() => expect(override).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 10));
    expect((screen.getByRole("button", { name: "Start recovery" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("uses the server the sign-in page chose", async () => {
    setSignInServer("https://family.example.net/");
    render(routed(RecoverAccount, "/recover"));
    await waitFor(() => expect(settings).toHaveBeenCalledWith("https://family.example.net"));
    expect(override).not.toHaveBeenCalled();
  });
});
