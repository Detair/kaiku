import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ createRecoveryCodes: vi.fn(), getMeSecurity: vi.fn() }));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

import RecoveryCodesSection from "../RecoveryCodesSection";
import { refreshMeSecurity } from "@/stores/security";
import { setAuthState } from "@/stores/auth";
import { security, signInAs } from "./securityTestUtils";

beforeEach(async () => {
  for (const f of Object.values(api)) f.mockReset();
  stepUp.mockClear();
  signInAs("full");
  api.getMeSecurity.mockResolvedValue(security({ recovery_codes_left: 3 }));
  await refreshMeSecurity();
});
afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

describe("RecoveryCodesSection", () => {
  it("recovery codes are shown once after a step-up", async () => {
    api.createRecoveryCodes.mockResolvedValue(["ABCDEFGHJKMNP", "QRSTVWXYZ2345"]);
    render(() => <RecoveryCodesSection />);
    expect(screen.getByText("3 recovery codes left.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Create new codes" }));
    expect(await screen.findByText("ABCDE FGHJK MNP")).toBeTruthy();
    expect(screen.getByText("Write these down. Each works once, only to recover your account.")).toBeTruthy();
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "recovery_codes.create", target: "" });
    fireEvent.click(screen.getByRole("button", { name: "I wrote them down" }));
    expect(screen.queryByText("ABCDE FGHJK MNP")).toBeNull();
  });
});
