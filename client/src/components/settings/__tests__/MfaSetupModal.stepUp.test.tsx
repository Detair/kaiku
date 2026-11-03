// TOTP setup (C27): setup checks the totp.set grant, verify spends it. A scan
// that outlasts the grant gets one new step-up and one retry (Argon 3 #1);
// failures are worded, never the server's text (3 #3).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ mfaSetup: vi.fn(), mfaVerify: vi.fn() }));
vi.mock("@/lib/tauri", async (orig) => ({ ...(await orig<object>()), ...api }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("qrcode", () => ({ default: { toDataURL: vi.fn(async () => "data:image/png;base64,AAAA") } }));

import MfaSetupModal from "../MfaSetupModal";
import { HttpError } from "@/lib/tauri/common";

beforeEach(() => {
  api.mfaSetup.mockReset().mockResolvedValue({ secret: "S", qr_code_url: "otpauth://totp/x" });
  api.mfaVerify.mockReset();
  stepUp.mockClear();
});
afterEach(cleanup);

async function toVerify() {
  render(() => <MfaSetupModal onClose={vi.fn()} onComplete={vi.fn()} />);
  const next = (await screen.findByRole("button", { name: "Next" })) as HTMLButtonElement;
  await waitFor(() => expect(next.disabled).toBe(false));
  fireEvent.click(next);
  fireEvent.input(screen.getByPlaceholderText("000000"), { target: { value: "123456" } });
  fireEvent.click(screen.getByRole("button", { name: /Verify/ }));
}

describe("MfaSetupModal step-up", () => {
  it("an expired totp.set grant at verify gets one new step-up and a retry", async () => {
    api.mfaVerify
      .mockRejectedValueOnce(new HttpError(403, "x", "STEP_UP_REQUIRED"))
      .mockResolvedValueOnce({ success: true, message: "ok" });
    await toVerify();
    await waitFor(() => expect(api.mfaVerify).toHaveBeenCalledTimes(2));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "totp.set" });
    expect(await screen.findByRole("button", { name: "Close" })).toBeTruthy();
  });

  it("a refused code shows our text, not the server's", async () => {
    api.mfaVerify.mockRejectedValue(new HttpError(400, "server says: bad", "INVALID_MFA_CODE"));
    await toVerify();
    await waitFor(() => expect(api.mfaVerify).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 0));
    expect(document.body.textContent).not.toContain("server says");
  });
});
