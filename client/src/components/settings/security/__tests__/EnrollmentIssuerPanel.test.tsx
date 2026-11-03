import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  issueEnrollment: vi.fn(),
  getEnrollment: vi.fn(),
  confirmEnrollment: vi.fn(),
  cancelEnrollment: vi.fn(),
}));
vi.mock("@/lib/tauri/enrollments", async (orig) => ({ ...(await orig<object>()), ...api }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));
const offer = vi.hoisted(() => vi.fn());
vi.mock("@/stores/security", async (orig) => ({ ...(await orig<object>()), offerSignOutOthers: offer }));

import EnrollmentIssuerPanel from "../EnrollmentIssuerPanel";
import { HttpError } from "@/lib/tauri/common";
import { ENROLLMENT_UPDATED_EVENT } from "@/lib/types/events";
import { setAuthState } from "@/stores/auth";
import { signInAs } from "./securityTestUtils";

const ID = "55555555-5555-4555-8555-555555555555";
const issued = {
  enrollment_id: ID,
  setup_token: "tok",
  setup_url: "https://chat.example.com/setup#v=1&t=tok",
  expires_at: new Date(Date.now() + 10 * 60_000).toISOString(),
};
function enrollment(state: string, over: Record<string, unknown> = {}) {
  return {
    id: ID,
    variant: "own",
    state,
    subject: { user_id: "me", username: "alice", display_name: "Alice" },
    claimed: state === "issued" ? null : { authenticator_type: "key_device", label: "Pixel 8", key_hash: "h" },
    tries_left: 3,
    code_expires_at: null,
    expires_at: issued.expires_at,
    request_id: null,
    ...over,
  };
}

const onDone = vi.fn();
const onCancel = vi.fn();

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  stepUp.mockClear();
  offer.mockReset();
  onDone.mockReset();
  onCancel.mockReset();
  api.issueEnrollment.mockResolvedValue(issued);
  api.getEnrollment.mockResolvedValue(enrollment("issued"));
  api.cancelEnrollment.mockResolvedValue(undefined);
  signInAs("full");
});
afterEach(() => {
  cleanup();
  setAuthState({ user: null });
  vi.useRealTimers();
});

function mount(kind: "own" | "request" = "own") {
  render(() => <EnrollmentIssuerPanel issuer={{ kind }} onDone={onDone} onCancel={onCancel} />);
}

async function claimed() {
  api.getEnrollment.mockResolvedValue(enrollment("claimed"));
  window.dispatchEvent(new CustomEvent(ENROLLMENT_UPDATED_EVENT, { detail: { enrollment_id: ID } }));
  return screen.findByText("A device calling itself 'Pixel 8' wants to become your key");
}

async function typeCode(code: string) {
  fireEvent.input(screen.getByLabelText("Code shown on the new device"), { target: { value: code } });
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
}

describe("EnrollmentIssuerPanel", () => {
  it("own issuer steps up before issuing", async () => {
    mount();
    await waitFor(() => expect(api.issueEnrollment).toHaveBeenCalledWith({ kind: "own" }));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "enrollment.issue", target: "" });
    expect(stepUp.mock.invocationCallOrder[0]).toBeLessThan(api.issueEnrollment.mock.invocationCallOrder[0]);
  });

  it("request issuer needs no step-up", async () => {
    mount("request");
    await waitFor(() => expect(api.issueEnrollment).toHaveBeenCalledWith({ kind: "request" }));
    expect(stepUp).not.toHaveBeenCalled();
  });

  it("QR shows the setup url and copy link copies it", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    mount();
    expect(await screen.findByAltText("Setup code for the new phone")).toBeTruthy();
    expect(document.body.textContent).not.toContain(issued.setup_url);
    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(issued.setup_url));
  });

  it("claimed state names the device calling itself", async () => {
    mount();
    await screen.findByAltText("Setup code for the new phone");
    expect(await claimed()).toBeTruthy();
    expect(screen.getByText("Type the code from the device in your hand. Never type a code someone tells you.")).toBeTruthy();
  });

  it("enrollment_updated hint refetches at once", async () => {
    mount();
    await screen.findByAltText("Setup code for the new phone");
    const before = api.getEnrollment.mock.calls.length;
    window.dispatchEvent(new CustomEvent(ENROLLMENT_UPDATED_EVENT, { detail: { enrollment_id: ID } }));
    await waitFor(() => expect(api.getEnrollment.mock.calls.length).toBe(before + 1));
    // Another enrolment's hint is ignored.
    window.dispatchEvent(new CustomEvent(ENROLLMENT_UPDATED_EVENT, { detail: { enrollment_id: "other" } }));
    await new Promise((r) => setTimeout(r, 20));
    expect(api.getEnrollment.mock.calls.length).toBe(before + 1);
  });

  it("polls every 3 s while waiting for the claim", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mount();
    await screen.findByAltText("Setup code for the new phone");
    const before = api.getEnrollment.mock.calls.length;
    await vi.advanceTimersByTimeAsync(3100);
    expect(api.getEnrollment.mock.calls.length).toBeGreaterThan(before);
  });

  it("wrong code shows tries left", async () => {
    mount();
    await screen.findByAltText("Setup code for the new phone");
    await claimed();
    api.confirmEnrollment.mockRejectedValueOnce(new HttpError(400, "x", "ENROLL_CODE_WRONG", { tries_left: 2 }));
    await typeCode("123456");
    expect(await screen.findByText("That code is not right. 2 tries left.")).toBeTruthy();
    expect(api.confirmEnrollment).toHaveBeenCalledWith({ kind: "own" }, ID, "123456");
  });

  it("third wrong code shows start again", async () => {
    mount();
    await screen.findByAltText("Setup code for the new phone");
    await claimed();
    api.confirmEnrollment.mockRejectedValueOnce(new HttpError(410, "x", "ENROLLMENT_FAILED"));
    await typeCode("123456");
    expect(await screen.findByText("Three wrong codes. Start again with a new setup code.")).toBeTruthy();
  });

  it("done offers sign out everywhere else", async () => {
    mount();
    await screen.findByAltText("Setup code for the new phone");
    await claimed();
    api.confirmEnrollment.mockResolvedValueOnce(enrollment("done"));
    await typeCode("123456");
    expect(await screen.findByText("'Pixel 8' is now your key.")).toBeTruthy();
    expect(offer).toHaveBeenCalled();
    expect(onDone).toHaveBeenCalled();
  });

  it("an expired code says so", async () => {
    mount();
    await screen.findByAltText("Setup code for the new phone");
    api.getEnrollment.mockResolvedValue(enrollment("expired"));
    window.dispatchEvent(new CustomEvent(ENROLLMENT_UPDATED_EVENT, { detail: { enrollment_id: ID } }));
    expect(await screen.findByText("This setup code expired.")).toBeTruthy();
  });

  it("too many open codes says to cancel one", async () => {
    api.issueEnrollment.mockRejectedValue(new HttpError(409, "x", "ENROLLMENT_LIMIT"));
    mount();
    expect(await screen.findByText("Too many open setup codes. Cancel one first.")).toBeTruthy();
  });

  it("cancel deletes the enrolment", async () => {
    mount();
    await screen.findByAltText("Setup code for the new phone");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(api.cancelEnrollment).toHaveBeenCalledWith({ kind: "own" }, ID));
    expect(onCancel).toHaveBeenCalled();
  });
});

describe("issuer panel review round (Argon review 3 #2)", () => {
  it("closing the panel gives an open code up", async () => {
    const view = render(() => <EnrollmentIssuerPanel issuer={{ kind: "own" }} onDone={onDone} onCancel={onCancel} />);
    await waitFor(() => expect(api.issueEnrollment).toHaveBeenCalled());
    await screen.findByRole("img");
    view.unmount();
    expect(api.cancelEnrollment).toHaveBeenCalledWith({ kind: "own" }, ID);
  });

  it("a code issued after the panel closed is cancelled at once", async () => {
    let finish!: (v: unknown) => void;
    api.issueEnrollment.mockImplementation(() => new Promise((r) => (finish = r)));
    const view = render(() => <EnrollmentIssuerPanel issuer={{ kind: "own" }} onDone={onDone} onCancel={onCancel} />);
    await waitFor(() => expect(api.issueEnrollment).toHaveBeenCalled());
    view.unmount();
    finish(issued);
    await waitFor(() => expect(api.cancelEnrollment).toHaveBeenCalledWith({ kind: "own" }, ID));
  });

  it("a code confirmed elsewhere (poll or hint) finishes like one typed here", async () => {
    render(() => <EnrollmentIssuerPanel issuer={{ kind: "own" }} onDone={onDone} onCancel={onCancel} />);
    await screen.findByRole("img");
    api.getEnrollment.mockResolvedValue(enrollment("done"));
    window.dispatchEvent(new CustomEvent(ENROLLMENT_UPDATED_EVENT, { detail: { enrollment_id: ID } }));
    await waitFor(() => expect(onDone).toHaveBeenCalledTimes(1));
    expect(offer).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new CustomEvent(ENROLLMENT_UPDATED_EVENT, { detail: { enrollment_id: ID } }));
    await new Promise((r) => setTimeout(r, 0));
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  it("a closed step-up closes the panel", async () => {
    const { StepUpCancelled } = await vi.importActual<typeof import("@/lib/stepUp")>("@/lib/stepUp");
    stepUp.mockImplementationOnce(async () => {
      throw new StepUpCancelled();
    });
    render(() => <EnrollmentIssuerPanel issuer={{ kind: "own" }} onDone={onDone} onCancel={onCancel} />);
    await waitFor(() => expect(onCancel).toHaveBeenCalled());
  });
});
