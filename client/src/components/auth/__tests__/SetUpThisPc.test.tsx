import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@solidjs/testing-library";

const getMeSecurity = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), getMeSecurity }));

const pc = vi.hoisted(() => ({
  startPcSetup: vi.fn(),
  currentPcSetup: vi.fn(),
  pollPcSetup: vi.fn(),
  cancelPcSetup: vi.fn(),
  enterPcSetupCode: vi.fn(),
  startPcSetupPasskey: vi.fn(),
  finishPcSetupPasskey: vi.fn(),
}));
vi.mock("@/lib/tauri/pcSetup", async (orig) => ({ ...(await orig<object>()), ...pc }));

const tauri = vi.hoisted(() => ({ on: false }));
vi.mock("@/lib/tauri/common", async (orig) => ({ ...(await orig<object>()), isTauri: () => tauri.on }));

const createPasskeyCredential = vi.hoisted(() => vi.fn());
const setUpThisPcInBrowser = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/passkeys", async (orig) => ({
  ...(await orig<object>()),
  createPasskeyCredential,
  setUpThisPcInBrowser,
  canUsePasskeys: () => true,
}));

const replaceSessionWithPasskey = vi.hoisted(() => vi.fn());
vi.mock("@/stores/auth", async (orig) => ({ ...(await orig<object>()), replaceSessionWithPasskey }));

import SetUpThisPc from "../SetUpThisPc";
import ProbationBanner from "../ProbationBanner";
import SetUpThisPcHost from "../SetUpThisPcHost";
import { closeSetUpThisPc } from "../SetUpThisPcButton";
import PasskeyFirstSteps from "@/components/settings/security/PasskeyFirstSteps";
import { Route, Router } from "@solidjs/router";
import { setAuthState } from "@/stores/auth";
import { refreshMeSecurity, resetMeSecurity } from "@/stores/security";
import { HttpError } from "@/lib/tauri/common";
import { PasskeyError } from "@/lib/tauri/passkeys";
import type { User } from "@/lib/types";
import { t } from "@/i18n";

const ID = "00000000-0000-4000-8000-000000000001";
const status = (state: string, reason: string | null = null, extra: object = {}) => ({
  attempt_id: ID,
  state,
  reason,
  ...extra,
});
const never = () => new Promise<never>(() => undefined);

function http(code: string, statusCode: number) {
  return new HttpError(statusCode, code, code, {});
}

const probation = {
  id: "s1",
  assurance: "probation",
  auth_methods: ["qr"],
  auth_time: "",
  client_class: "web",
  absolute_expires_at: "",
  key_device_id: null,
};
function signIn(session: object = probation) {
  setAuthState({ user: { id: "u1", username: "alice", session } as unknown as User });
}
function security(passkeys: number, can: boolean) {
  return {
    session: { assurance: "probation", auth_methods: ["qr"] },
    password: null,
    totp: null,
    recovery_codes_left: 0,
    key_devices: can ? 1 : 0,
    passkeys,
    may_approve: can,
    can_set_up_this_pc: can,
    bootstrap_open: false,
    recovery_contact: null,
    open_request: null,
    prompts: [],
  };
}

/** The server's answers to the polls, one per call; later polls hang. */
function polls(...answers: object[]) {
  pc.pollPcSetup.mockReset();
  for (const a of answers) pc.pollPcSetup.mockResolvedValueOnce(a);
  pc.pollPcSetup.mockImplementation(never);
}

beforeEach(() => {
  for (const f of Object.values(pc)) f.mockReset();
  createPasskeyCredential.mockReset();
  setUpThisPcInBrowser.mockReset();
  tauri.on = false;
  replaceSessionWithPasskey.mockReset();
  getMeSecurity.mockReset();
  resetMeSecurity();
  pc.currentPcSetup.mockRejectedValue(http("APPROVAL_NOT_FOUND", 404));
  pc.startPcSetup.mockResolvedValue({ attempt_id: ID, qr_payload: "kaiku://pcsetup/v1?o=x&t=y", rotates_at: "" });
  pc.cancelPcSetup.mockResolvedValue(undefined);
  polls();
  signIn();
});

afterEach(() => {
  cleanup();
  closeSetUpThisPc();
  vi.useRealTimers();
  setAuthState({ user: null });
});

async function open() {
  const onClose = vi.fn();
  render(() => <SetUpThisPc onClose={onClose} />);
  await screen.findByRole("button", { name: "Start again" });
  return onClose;
}

/** Open the dialog with the poll answering `state` (after `waiting`). */
async function openAt(state: string, reason: string | null = null) {
  polls(status(state, reason));
  render(() => <SetUpThisPc onClose={vi.fn()} />);
  await waitFor(() => expect(pc.pollPcSetup).toHaveBeenCalled());
}

describe("buttons in the banners", () => {
  it("shows Set up this PC only with can_set_up_this_pc in a chat-only session", async () => {
    getMeSecurity.mockResolvedValue(security(0, true));
    render(() => <ProbationBanner />);
    expect(await screen.findByRole("button", { name: "Set up this PC" })).toBeTruthy();
    cleanup();
    resetMeSecurity();
    getMeSecurity.mockResolvedValue(security(0, false));
    render(() => <ProbationBanner />);
    await screen.findByText("Your phone can't confirm this yet. Ask your admin.");
    expect(screen.queryByRole("button", { name: "Set up this PC" })).toBeNull();
  });

  it("with an active passkey, Sign in with a passkey comes first", async () => {
    getMeSecurity.mockResolvedValue(security(1, true));
    render(() => <ProbationBanner />);
    const set = await screen.findByRole("button", { name: "Set up this PC" });
    const sign = screen.getByRole("button", { name: "Sign in with a passkey" });
    expect(sign.compareDocumentPosition(set) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("a full session gets no button", async () => {
    signIn({ ...probation, assurance: "full" });
    getMeSecurity.mockResolvedValue(security(0, true));
    await refreshMeSecurity();
    render(() => <ProbationBanner />);
    expect(screen.queryByRole("button", { name: "Set up this PC" })).toBeNull();
  });

  it("the empty passkey list offers it too", async () => {
    getMeSecurity.mockResolvedValue(security(0, true));
    await refreshMeSecurity();
    render(() => (
      <Router>
        <Route path="/" component={() => <PasskeyFirstSteps onSetUpPhone={vi.fn()} />} />
      </Router>
    ));
    expect(screen.getByRole("button", { name: "Set up this PC" })).toBeTruthy();
  });

  it("clicking it opens the dialog and starts an attempt", async () => {
    getMeSecurity.mockResolvedValue(security(0, true));
    render(() => (
      <>
        <ProbationBanner />
        <SetUpThisPcHost />
      </>
    ));
    fireEvent.click(await screen.findByRole("button", { name: "Set up this PC" }));
    await waitFor(() => expect(pc.startPcSetup).toHaveBeenCalledOnce());
    expect(await screen.findByRole("img", { name: "Code to scan with the Kaiku app on your phone" })).toBeTruthy();
  });
});

describe("the dialog", () => {
  it("starts an attempt and shows its QR code", async () => {
    await open();
    expect(await screen.findByRole("img", { name: "Code to scan with the Kaiku app on your phone" })).toBeTruthy();
  });

  it("resumes the session's attempt on mount", async () => {
    pc.currentPcSetup.mockResolvedValue(status("claimed"));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    expect(await screen.findByText(/Confirm on your phone to continue/)).toBeTruthy();
    expect(pc.startPcSetup).not.toHaveBeenCalled();
    expect(pc.pollPcSetup).toHaveBeenCalledWith(ID, expect.any(Number), expect.any(AbortSignal));
  });

  it("a start the server refuses says to ask the admin", async () => {
    pc.currentPcSetup.mockRejectedValue(http("APPROVAL_NOT_FOUND", 404));
    pc.startPcSetup.mockRejectedValue(http("PC_SETUP_UNAVAILABLE", 403));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    expect(await screen.findByText("Your phone can't confirm this yet. Ask your admin.")).toBeTruthy();
  });

  it("never polls closer than a second, even when the server answers at once", async () => {
    vi.useFakeTimers();
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockResolvedValue(status("waiting"));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(900);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(100);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1000);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(3);
  });

  it("closing the dialog cancels a live attempt", async () => {
    await open();
    cleanup();
    expect(pc.cancelPcSetup).toHaveBeenCalledWith(ID);
  });
});

describe("the PC error table", () => {
  const cases: [string, string | null, string][] = [
    ["refused", "refused", "Your phone declined. Start again?"],
    ["expired", "may_approve_off", "Your phone can no longer confirm this. Ask your admin."],
    ["expired", "device_revoked", "Your phone can no longer confirm this. Ask your admin."],
    ["expired", "timeout", "This took too long. Start again?"],
    ["expired", "enrollment_expired", "This took too long. Start again?"],
    ["expired", "code_expired", "This took too long. Start again?"],
    ["expired", "session_ended", "This session has ended. Sign in again."],
    ["expired", "no_key_device", "Your phone can no longer confirm this. Ask your admin."],
    ["expired", "account_banned", "This account has been suspended. Ask the server admin."],
    ["reported", "reported", "Your phone reported this. Nothing was set up on this computer."],
    ["failed", "too_many_codes", "Too many wrong codes. Start again?"],
    ["cancelled", "restarted", "This was cancelled. Start again?"],
  ];
  for (const [state, reason, text] of cases) {
    it(`${state} / ${reason}: ${text}`, async () => {
      await openAt(state, reason);
      expect(await screen.findByText(text)).toBeTruthy();
      expect(screen.getByRole("button", { name: "Start again" })).toBeTruthy();
    });
  }

  it("a vanished attempt (404) is ended, with Start again", async () => {
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockRejectedValueOnce(http("APPROVAL_NOT_FOUND", 404));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    expect(await screen.findByText("This setup is no longer active. Start again?")).toBeTruthy();
  });
});

describe("Start again", () => {
  for (const state of ["waiting", "claimed", "code", "enrolled", "completed", "refused", "expired"]) {
    it(`is there in ${state} and restarts`, async () => {
      if (state === "waiting") await open();
      else await openAt(state, state === "expired" ? "timeout" : null);
      await waitFor(() => expect(screen.getByRole("button", { name: "Start again" }).hasAttribute("disabled")).toBe(false));
      pc.startPcSetup.mockClear();
      fireEvent.click(screen.getByRole("button", { name: "Start again" }));
      await waitFor(() => expect(pc.startPcSetup).toHaveBeenCalledOnce());
    });
  }
});

describe("code and passkey on the web", () => {
  async function typeCode(digits: string) {
    fireEvent.input(await screen.findByLabelText("Code from your phone"), { target: { value: digits } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  }

  it("a wrong code shows the tries left and keeps asking", async () => {
    await openAt("code");
    pc.enterPcSetupCode.mockResolvedValue({ state: "code", tries_left: 2 });
    await typeCode("123456");
    expect(await screen.findByText("That code is wrong. 2 tries left.")).toBeTruthy();
    expect(pc.enterPcSetupCode).toHaveBeenCalledWith(ID, "123456");
    expect(screen.getByLabelText("Code from your phone")).toBeTruthy();
  });

  it("the last try left is worded in the singular", async () => {
    await openAt("code");
    pc.enterPcSetupCode.mockResolvedValue({ state: "code", tries_left: 1 });
    await typeCode("123456");
    expect(await screen.findByText("That code is wrong. 1 try left.")).toBeTruthy();
  });

  it("the third wrong code ends the attempt", async () => {
    await openAt("code");
    pc.enterPcSetupCode.mockResolvedValue({ state: "failed", tries_left: 0 });
    await typeCode("123456");
    expect(await screen.findByText("Too many wrong codes. Start again?")).toBeTruthy();
  });

  it("an expired code says it took too long", async () => {
    await openAt("code");
    pc.enterPcSetupCode.mockRejectedValue(http("APPROVAL_EXPIRED", 410));
    await typeCode("123456");
    expect(await screen.findByText("This took too long. Start again?")).toBeTruthy();
  });

  it("the right code leads to the passkey, then to sign in with it", async () => {
    await openAt("code");
    pc.enterPcSetupCode.mockResolvedValue({ state: "enrolled", enrollment_id: "e1" });
    pc.startPcSetupPasskey.mockResolvedValue({ ceremony_id: "c1", options: { publicKey: { challenge: "x" } } });
    createPasskeyCredential.mockResolvedValue({ id: "cred" });
    pc.finishPcSetupPasskey.mockResolvedValue({});
    replaceSessionWithPasskey.mockResolvedValue({});
    await typeCode("123456");
    fireEvent.click(await screen.findByRole("button", { name: "Create a passkey" }));
    await waitFor(() =>
      // Named like every passkey made in a browser ("Firefox on Linux").
      expect(pc.finishPcSetupPasskey).toHaveBeenCalledWith(ID, "c1", { id: "cred" }, expect.stringMatching(/\S/)),
    );
    expect(createPasskeyCredential).toHaveBeenCalledWith({ publicKey: { challenge: "x" } }, expect.anything());
    fireEvent.click(await screen.findByRole("button", { name: "Sign in with your passkey" }));
    await waitFor(() => expect(replaceSessionWithPasskey).toHaveBeenCalledOnce());
  });

  it("resumes at the passkey step when the poll says enrolled", async () => {
    await openAt("enrolled");
    expect(await screen.findByRole("button", { name: "Create a passkey" })).toBeTruthy();
  });

  it("a passkey limit is worded and the step can be tried again", async () => {
    await openAt("enrolled");
    pc.startPcSetupPasskey.mockRejectedValue(http("PASSKEY_LIMIT", 409));
    fireEvent.click(await screen.findByRole("button", { name: "Create a passkey" }));
    expect(await screen.findByText("You already have 10 passkeys. Remove one first.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Create a passkey" })).toBeTruthy();
  });

  it("an already registered passkey is worded", async () => {
    await openAt("enrolled");
    pc.startPcSetupPasskey.mockResolvedValue({ ceremony_id: "c1", options: {} });
    createPasskeyCredential.mockResolvedValue({});
    pc.finishPcSetupPasskey.mockRejectedValue(http("PASSKEY_ALREADY_REGISTERED", 409));
    fireEvent.click(await screen.findByRole("button", { name: "Create a passkey" }));
    expect(await screen.findByText("This passkey is already added.")).toBeTruthy();
  });

  it("a browser that cannot make the passkey says so in the existing words", async () => {
    await openAt("enrolled");
    pc.startPcSetupPasskey.mockResolvedValue({ ceremony_id: "c1", options: {} });
    createPasskeyCredential.mockRejectedValue(new PasskeyError("PASSKEY_NOT_SUPPORTED"));
    fireEvent.click(await screen.findByRole("button", { name: "Create a passkey" }));
    expect((await screen.findByRole("alert")).textContent).toBe(t("passkeys.error.notSupported"));
    expect(pc.finishPcSetupPasskey).not.toHaveBeenCalled();
  });
});

describe("the long-poll is closed", () => {
  /** Polls that stay open until their signal aborts, like the real request. */
  function openPolls() {
    const open = new Set<AbortSignal>();
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockImplementation(
      (_id: string, _wait: number, signal?: AbortSignal) =>
        new Promise((_, reject) => {
          if (!signal) return;
          open.add(signal);
          signal.addEventListener("abort", () => {
            open.delete(signal);
            reject(new DOMException("aborted", "AbortError"));
          });
        }),
    );
    return open;
  }

  it("no poll request stays open after unmount", async () => {
    const open = openPolls();
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await waitFor(() => expect(open.size).toBe(1));
    cleanup();
    expect(open.size).toBe(0);
  });

  it("no poll request stays open after Start again", async () => {
    const open = openPolls();
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await waitFor(() => expect(open.size).toBe(1));
    const first = [...open][0];
    pc.startPcSetup.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Start again" }));
    await waitFor(() => expect(first.aborted).toBe(true));
    await waitFor(() => expect(pc.startPcSetup).toHaveBeenCalledOnce());
    // Only the new attempt's poll is open.
    await waitFor(() => expect(open.size).toBe(1));
    expect(open.has(first)).toBe(false);
  });
});

describe("on the desktop", () => {
  beforeEach(() => {
    tauri.on = true;
  });

  it("shows no keypad, says to type the code in the browser, and waits for the click", async () => {
    await openAt("code");
    expect(await screen.findByText("Type the code in your browser")).toBeTruthy();
    expect(screen.queryByLabelText("Code from your phone")).toBeNull();
    expect(setUpThisPcInBrowser).not.toHaveBeenCalled();
  });

  it("opening the browser drives the pc_setup flow and ends on the sign-in step", async () => {
    await openAt("code");
    let finish: () => void = () => undefined;
    setUpThisPcInBrowser.mockImplementation(() => new Promise<void>((r) => (finish = r)));
    fireEvent.click(await screen.findByRole("button", { name: "Open the browser" }));
    expect(setUpThisPcInBrowser).toHaveBeenCalledWith(ID, expect.any(AbortSignal));
    await screen.findByText(/Waiting for your browser/);
    finish();
    expect(await screen.findByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
  });

  it("the flow survives the switch from code to enrolled and ends with the dialog", async () => {
    polls(status("code"), status("enrolled"));
    let signal: AbortSignal | undefined;
    setUpThisPcInBrowser.mockImplementation((_id: string, s: AbortSignal) => {
      signal = s;
      return never();
    });
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Open the browser" }));
    await screen.findByText("The code was right. Now create a passkey on this computer.");
    expect(signal?.aborted).toBe(false);
    cleanup();
    await Promise.resolve();
    expect(signal?.aborted).toBe(true);
  });

  it("a refused redeem after the code shows the existing passkey error text", async () => {
    // A redeem refusal arrives when the code was typed: the step is `enrolled`.
    polls(status("code"), status("enrolled"));
    let refuse: (e: unknown) => void = () => undefined;
    setUpThisPcInBrowser.mockImplementation(() => new Promise((_, reject) => (refuse = reject)));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Open the browser" }));
    await screen.findByText("The code was right. Now create a passkey on this computer.");
    refuse(new PasskeyError("PASSKEY_LIMIT"));
    expect(await screen.findByText("You already have 10 passkeys. Remove one first.")).toBeTruthy();
    // One line: the passkey text, not the "interrupted" line as well.
    expect(screen.queryByText("The browser step was interrupted. Use Start again.")).toBeNull();
  });

  it("a flow that failed in state code can be opened again", async () => {
    await openAt("code");
    setUpThisPcInBrowser.mockRejectedValueOnce(new PasskeyError("PASSKEY_CANCELLED"));
    fireEvent.click(await screen.findByRole("button", { name: "Open the browser" }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    setUpThisPcInBrowser.mockImplementation(never);
    fireEvent.click(screen.getByRole("button", { name: "Open the browser" }));
    await waitFor(() => expect(setUpThisPcInBrowser).toHaveBeenCalledTimes(2));
  });

  it("after a restart, an enrolled attempt starts no flow and offers Start again", async () => {
    pc.currentPcSetup.mockResolvedValue(status("enrolled", null, { enrollment_id: "e1" }));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    expect(await screen.findByText("The browser step was interrupted. Use Start again.")).toBeTruthy();
    expect(setUpThisPcInBrowser).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Create a passkey" })).toBeNull();
    pc.startPcSetup.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Start again" }));
    await waitFor(() => expect(pc.startPcSetup).toHaveBeenCalledOnce());
  });

  it("after a restart, a code attempt starts no flow until the person opens the browser", async () => {
    pc.currentPcSetup.mockResolvedValue(status("code"));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    expect(await screen.findByRole("button", { name: "Open the browser" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Start again" })).toBeTruthy();
    expect(setUpThisPcInBrowser).not.toHaveBeenCalled();
  });
});

describe("answers belong to their attempt", () => {
  const OLD = ID;
  const NEW = "00000000-0000-4000-8000-000000000002";

  it("a late code answer of the old attempt does not move the new one", async () => {
    await openAt("code");
    let answer: (v: unknown) => void = () => undefined;
    pc.enterPcSetupCode.mockImplementation(() => new Promise((r) => (answer = r)));
    fireEvent.input(await screen.findByLabelText("Code from your phone"), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(pc.enterPcSetupCode).toHaveBeenCalledWith(OLD, "123456"));
    pc.startPcSetup.mockResolvedValue({ attempt_id: NEW, qr_payload: "kaiku://pcsetup/v1?o=x&t=z", rotates_at: "" });
    // The old attempt's end is read before the new code shows (R-70).
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockImplementation((id: string, wait: number) =>
      id === OLD && wait === 0 ? Promise.resolve(status("cancelled", "restarted")) : never(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Start again" }));
    expect(await screen.findByRole("img", { name: "Code to scan with the Kaiku app on your phone" })).toBeTruthy();
    answer({ state: "enrolled", enrollment_id: "e1" });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByRole("button", { name: "Create a passkey" })).toBeNull();
    expect(screen.getByRole("img", { name: "Code to scan with the Kaiku app on your phone" })).toBeTruthy();
  });

  it("a late poll answer never takes back a completed attempt", async () => {
    let late: (v: unknown) => void = () => undefined;
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup
      .mockResolvedValueOnce(status("enrolled"))
      .mockImplementationOnce(() => new Promise((r) => (late = r)))
      .mockImplementation(never);
    pc.startPcSetupPasskey.mockResolvedValue({ ceremony_id: "c1", options: {} });
    createPasskeyCredential.mockResolvedValue({});
    pc.finishPcSetupPasskey.mockResolvedValue({});
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    const create = await screen.findByRole("button", { name: "Create a passkey" });
    // A poll is open while the passkey is made.
    await waitFor(() => expect(pc.pollPcSetup).toHaveBeenCalledTimes(2), { timeout: 2_000 });
    fireEvent.click(create);
    expect(await screen.findByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
    // That poll answers only now, with what it read before the finish.
    late(status("enrolled"));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
  });
});

describe("C27's generic text and poll errors", () => {
  it("an unknown reason or state reads as the generic text, not as a timeout", async () => {
    await openAt("expired", "some_new_reason");
    expect(await screen.findByText("This didn't work. Start again.")).toBeTruthy();
    cleanup();
    await openAt("some_new_state", null);
    expect(await screen.findByText("This didn't work. Start again.")).toBeTruthy();
  });

  it("a 429 waits as long as Retry-After says", async () => {
    vi.useFakeTimers();
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup
      .mockRejectedValueOnce(new HttpError(429, "TOO_MANY_ATTEMPTS", "TOO_MANY_ATTEMPTS", {}, 12))
      .mockImplementation(never);
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(11_000);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1_500);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(2);
  });

  it("failed polls back off", async () => {
    vi.useFakeTimers();
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockRejectedValue(new Error("offline"));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(3_000);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(2);
    // The second pause is longer than the first.
    await vi.advanceTimersByTimeAsync(3_000);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(3_000);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(3);
  });

  it("a refused poll ends with the generic text instead of retrying", async () => {
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockRejectedValueOnce(http("FORBIDDEN", 403)).mockImplementation(never);
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    expect(await screen.findByText("This didn't work. Start again.")).toBeTruthy();
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(1);
  });
});

describe("the attempt follows the dialog", () => {
  it("closing during the start cancels the attempt the server made", async () => {
    let answer: (v: unknown) => void = () => undefined;
    pc.startPcSetup.mockImplementation(() => new Promise((r) => (answer = r)));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await waitFor(() => expect(pc.startPcSetup).toHaveBeenCalled());
    cleanup();
    answer({ attempt_id: ID, qr_payload: "kaiku://pcsetup/v1?o=x&t=y", rotates_at: "" });
    await waitFor(() => expect(pc.cancelPcSetup).toHaveBeenCalledWith(ID));
  });

  it("a refresh of the security overview does not close an open dialog", async () => {
    getMeSecurity.mockResolvedValue(security(0, true));
    render(() => (
      <>
        <ProbationBanner />
        <SetUpThisPcHost />
      </>
    ));
    fireEvent.click(await screen.findByRole("button", { name: "Set up this PC" }));
    expect(await screen.findByRole("img", { name: "Code to scan with the Kaiku app on your phone" })).toBeTruthy();
    getMeSecurity.mockResolvedValue(security(0, false));
    await refreshMeSecurity();
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByRole("img", { name: "Code to scan with the Kaiku app on your phone" })).toBeTruthy();
    expect(pc.cancelPcSetup).not.toHaveBeenCalled();
  });
});

describe("the dialog belongs to one signed-in chat-only session", () => {
  async function openFromBanner() {
    getMeSecurity.mockResolvedValue(security(0, true));
    render(() => (
      <>
        <ProbationBanner />
        <SetUpThisPcHost />
      </>
    ));
    fireEvent.click(await screen.findByRole("button", { name: "Set up this PC" }));
    expect(await screen.findByRole("img", { name: "Code to scan with the Kaiku app on your phone" })).toBeTruthy();
    expect(pc.startPcSetup).toHaveBeenCalledOnce();
  }

  it("a session end with the dialog open leaves no dialog on the signed-out page", async () => {
    await openFromBanner();
    setAuthState({ user: null });
    await waitFor(() => expect(screen.queryByTestId("set-up-this-pc")).toBeNull());
  });

  it("the next chat-only sign-in in this window gets no dialog and starts nothing", async () => {
    await openFromBanner();
    setAuthState({ user: null });
    await waitFor(() => expect(screen.queryByTestId("set-up-this-pc")).toBeNull());
    pc.startPcSetup.mockClear();
    pc.currentPcSetup.mockClear();
    // Another account signs in for chat only.
    setAuthState({
      user: { id: "u2", username: "bob", session: { ...probation, id: "s2" } } as unknown as User,
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByTestId("set-up-this-pc")).toBeNull();
    expect(pc.startPcSetup).not.toHaveBeenCalled();
    expect(pc.currentPcSetup).not.toHaveBeenCalled();
  });
});

describe("answers that a retry cannot change, and ones it can", () => {
  it("a refused passkey step says to start again, not to try again", async () => {
    await openAt("enrolled");
    pc.startPcSetupPasskey.mockRejectedValue(http("APPROVAL_STATE", 409));
    fireEvent.click(await screen.findByRole("button", { name: "Create a passkey" }));
    expect(await screen.findByText("This didn't work. Start again.")).toBeTruthy();
    expect(screen.queryByText("The passkey could not be created. Please try again.")).toBeNull();
  });

  it("a 401 from a refresh that failed for now is retried, not the end", async () => {
    vi.useFakeTimers();
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockRejectedValueOnce(http("INVALID_TOKEN", 401)).mockImplementation(never);
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await vi.advanceTimersByTimeAsync(0);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(3_100);
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("This didn't work. Start again.")).toBeNull();
  });
});

describe("late answers after the step or the dialog is gone", () => {
  it("a code answer after the attempt ended keeps the real end", async () => {
    let answer: (v: unknown) => void = () => undefined;
    let ended: (v: unknown) => void = () => undefined;
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup
      .mockResolvedValueOnce(status("code"))
      .mockImplementationOnce(() => new Promise((r) => (ended = r)))
      .mockImplementation(never);
    pc.enterPcSetupCode.mockImplementation(() => new Promise((r) => (answer = r)));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    fireEvent.input(await screen.findByLabelText("Code from your phone"), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(pc.pollPcSetup).toHaveBeenCalledTimes(2), { timeout: 2_000 });
    ended(status("expired", "may_approve_off"));
    expect(await screen.findByText("Your phone can no longer confirm this. Ask your admin.")).toBeTruthy();
    answer({ state: "enrolled", enrollment_id: "e1" });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText("Your phone can no longer confirm this. Ask your admin.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Create a passkey" })).toBeNull();
  });

  it("closing while the session's attempt is being looked up cancels it", async () => {
    let answer: (v: unknown) => void = () => undefined;
    pc.currentPcSetup.mockImplementation(() => new Promise((r) => (answer = r)));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await waitFor(() => expect(pc.currentPcSetup).toHaveBeenCalled());
    cleanup();
    answer(status("waiting", null, { qr_payload: "kaiku://pcsetup/v1?o=x&t=y" }));
    await waitFor(() => expect(pc.cancelPcSetup).toHaveBeenCalledWith(ID));
  });
});

describe("desktop texts", () => {
  beforeEach(() => {
    tauri.on = true;
  });

  it("an unknown flow error reads as the browser step, not as a passkey error", async () => {
    await openAt("code");
    setUpThisPcInBrowser.mockRejectedValueOnce(new PasskeyError("APPROVAL_EXPIRED"));
    fireEvent.click(await screen.findByRole("button", { name: "Open the browser" }));
    expect(await screen.findByText("The browser step did not finish. Please try again.")).toBeTruthy();
  });

  it("after the code a failed browser step shows one line: start again", async () => {
    polls(status("code"), status("enrolled"));
    setUpThisPcInBrowser.mockImplementation(
      () => new Promise((_, reject) => setTimeout(() => reject(new PasskeyError("PASSKEY_CANCELLED")), 50)),
    );
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Open the browser" }));
    expect(await screen.findByText("The browser step was interrupted. Use Start again.")).toBeTruthy();
    expect(screen.queryByText("The browser step did not finish. Please try again.")).toBeNull();
  });
});

describe("an end is final", () => {
  it("a failed poll arriving after the passkey was made keeps the sign-in step", async () => {
    let late: (e: unknown) => void = () => undefined;
    let lateSignal: AbortSignal | undefined;
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup
      .mockResolvedValueOnce(status("enrolled"))
      .mockImplementationOnce((_id: string, _w: number, signal?: AbortSignal) => {
        lateSignal = signal;
        return new Promise((_, reject) => (late = reject));
      })
      .mockImplementation(never);
    pc.startPcSetupPasskey.mockResolvedValue({ ceremony_id: "c1", options: {} });
    createPasskeyCredential.mockResolvedValue({});
    pc.finishPcSetupPasskey.mockResolvedValue({});
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    const create = await screen.findByRole("button", { name: "Create a passkey" });
    await waitFor(() => expect(pc.pollPcSetup).toHaveBeenCalledTimes(2), { timeout: 2_000 });
    fireEvent.click(create);
    expect(await screen.findByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
    // Entering the end closed the open poll.
    expect(lateSignal?.aborted).toBe(true);
    // Its refusal arrives anyway: the end stays.
    late(http("FORBIDDEN", 403));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
    expect(screen.queryByText("This didn't work. Start again.")).toBeNull();
  });

  it("no answer about the same attempt replaces an end", async () => {
    polls(status("expired", "timeout"), status("failed", "too_many_codes"));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    expect(await screen.findByText("This took too long. Start again?")).toBeTruthy();
    await new Promise((r) => setTimeout(r, 1_200));
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(1);
    expect(screen.getByText("This took too long. Start again?")).toBeTruthy();
  });
});

describe("unknown reasons (C27)", () => {
  it("an end with an unknown reason reads as the generic text, whatever its state", async () => {
    for (const state of ["failed", "refused", "reported", "cancelled"]) {
      await openAt(state, "some_new_reason");
      expect(await screen.findByText("This didn't work. Start again.")).toBeTruthy();
      cleanup();
    }
  });

  it("a live state with an unknown reason ends the polling with the generic text", async () => {
    await openAt("code", "some_new_reason");
    expect(await screen.findByText("This didn't work. Start again.")).toBeTruthy();
    expect(screen.queryByLabelText("Code from your phone")).toBeNull();
    await new Promise((r) => setTimeout(r, 1_200));
    expect(pc.pollPcSetup).toHaveBeenCalledTimes(1);
  });
});

describe("Start again against a finish", () => {
  const NEW = "00000000-0000-4000-8000-000000000002";

  it("reports completion when the finish won, and gives the new attempt up", async () => {
    let first = true;
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockImplementation((id: string, wait: number) => {
      if (first) {
        first = false;
        return Promise.resolve(status("enrolled"));
      }
      // The old attempt, asked once more after the restart.
      if (id === ID && wait === 0) return Promise.resolve(status("completed"));
      return never();
    });
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await screen.findByRole("button", { name: "Create a passkey" });
    pc.startPcSetup.mockResolvedValue({ attempt_id: NEW, qr_payload: "kaiku://pcsetup/v1?o=x&t=z", rotates_at: "" });
    fireEvent.click(screen.getByRole("button", { name: "Start again" }));
    expect(await screen.findByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
    expect(screen.queryByRole("img", { name: "Code to scan with the Kaiku app on your phone" })).toBeNull();
    await waitFor(() => expect(pc.cancelPcSetup).toHaveBeenCalledWith(NEW));
  });

  it("shows the new code when the restart won", async () => {
    let first = true;
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockImplementation((id: string, wait: number) => {
      if (first) {
        first = false;
        return Promise.resolve(status("enrolled"));
      }
      if (id === ID && wait === 0) return Promise.resolve(status("cancelled", "restarted"));
      return never();
    });
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await screen.findByRole("button", { name: "Create a passkey" });
    pc.startPcSetup.mockResolvedValue({ attempt_id: NEW, qr_payload: "kaiku://pcsetup/v1?o=x&t=z", rotates_at: "" });
    fireEvent.click(screen.getByRole("button", { name: "Start again" }));
    expect(await screen.findByRole("img", { name: "Code to scan with the Kaiku app on your phone" })).toBeTruthy();
    expect(pc.cancelPcSetup).not.toHaveBeenCalledWith(NEW);
  });
});

describe("an end the client made up gives way to the server's", () => {
  it("a refused poll during the passkey step does not hide the passkey the finish created", async () => {
    let refuse: (e: unknown) => void = () => undefined;
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup
      .mockResolvedValueOnce(status("enrolled"))
      .mockImplementationOnce(() => new Promise((_, reject) => (refuse = reject)))
      .mockImplementation(never);
    let credential: (v: unknown) => void = () => undefined;
    pc.startPcSetupPasskey.mockResolvedValue({ ceremony_id: "c1", options: {} });
    createPasskeyCredential.mockImplementation(() => new Promise((r) => (credential = r)));
    pc.finishPcSetupPasskey.mockResolvedValue({});
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    const create = await screen.findByRole("button", { name: "Create a passkey" });
    await waitFor(() => expect(pc.pollPcSetup).toHaveBeenCalledTimes(2), { timeout: 2_000 });
    fireEvent.click(create);
    await waitFor(() => expect(createPasskeyCredential).toHaveBeenCalled());
    // A proxy refuses one poll: the client ends the attempt on its own.
    refuse(http("FORBIDDEN", 403));
    expect(await screen.findByText("This didn't work. Start again.")).toBeTruthy();
    // The ceremony and the finish still succeed on the server.
    credential({});
    expect(await screen.findByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
  });
});

describe("R-70: a new code only once the earlier attempt's end is known", () => {
  const NEW = "00000000-0000-4000-8000-000000000002";
  const QR = { name: "Code to scan with the Kaiku app on your phone" };

  /** Open at `state`; the old attempt's read (wait 0) is answered by `old`. */
  function oldReads(state: string, reason: string | null, old: () => Promise<unknown>) {
    let first = true;
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup.mockImplementation((id: string, wait: number) => {
      if (first) {
        first = false;
        return Promise.resolve(status(state, reason));
      }
      if (id === ID && wait === 0) return old();
      return never();
    });
  }
  const nextStart = () =>
    pc.startPcSetup.mockResolvedValue({ attempt_id: NEW, qr_payload: "kaiku://pcsetup/v1?o=x&t=z", rotates_at: "" });

  it("after Start again from enrolled the code is not shown until the old attempt was read", async () => {
    let answer: (v: unknown) => void = () => undefined;
    oldReads("enrolled", null, () => new Promise((r) => (answer = r)));
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await screen.findByRole("button", { name: "Create a passkey" });
    fireEvent.click(screen.getByRole("button", { name: "Start again" }));
    expect(await screen.findByText("Checking the earlier setup…")).toBeTruthy();
    expect(screen.queryByRole("img", QR)).toBeNull();
    answer(status("cancelled", "restarted"));
    expect(await screen.findByRole("img", QR)).toBeTruthy();
  });

  it("three failed reads offer a retry, never the code", async () => {
    let reads = 0;
    oldReads("code", null, () => {
      reads += 1;
      return reads <= 3 ? Promise.reject(new Error("offline")) : Promise.resolve(status("cancelled", "restarted"));
    });
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await screen.findByLabelText("Code from your phone");
    nextStart();
    fireEvent.click(screen.getByRole("button", { name: "Start again" }));
    const retry = await screen.findByRole("button", { name: "Try again" }, { timeout: 5_000 });
    expect(screen.queryByRole("img", QR)).toBeNull();
    expect(reads).toBe(3);
    fireEvent.click(retry);
    expect(await screen.findByRole("img", QR)).toBeTruthy();
  });

  it("Start again from an end the client made up reads the old attempt", async () => {
    pc.pollPcSetup.mockReset();
    let first = true;
    pc.pollPcSetup.mockImplementation((id: string, wait: number) => {
      if (first) {
        first = false;
        return Promise.reject(http("FORBIDDEN", 403));
      }
      if (id === ID && wait === 0) return Promise.resolve(status("completed"));
      return never();
    });
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    await screen.findByText("This didn't work. Start again.");
    pc.startPcSetup.mockResolvedValue({ attempt_id: NEW, qr_payload: "kaiku://pcsetup/v1?o=x&t=z", rotates_at: "" });
    fireEvent.click(screen.getByRole("button", { name: "Start again" }));
    expect(await screen.findByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
    expect(pc.pollPcSetup).toHaveBeenCalledWith(ID, 0);
    await waitFor(() => expect(pc.cancelPcSetup).toHaveBeenCalledWith(NEW));
  });
});

describe("R-72: only the server's own not-found ends an attempt", () => {
  it("a 404 without the contract's code does not hide a later finish", async () => {
    let refuse: (e: unknown) => void = () => undefined;
    pc.pollPcSetup.mockReset();
    pc.pollPcSetup
      .mockResolvedValueOnce(status("enrolled"))
      .mockImplementationOnce(() => new Promise((_, reject) => (refuse = reject)))
      .mockImplementation(never);
    let credential: (v: unknown) => void = () => undefined;
    pc.startPcSetupPasskey.mockResolvedValue({ ceremony_id: "c1", options: {} });
    createPasskeyCredential.mockImplementation(() => new Promise((r) => (credential = r)));
    pc.finishPcSetupPasskey.mockResolvedValue({});
    render(() => <SetUpThisPc onClose={vi.fn()} />);
    const create = await screen.findByRole("button", { name: "Create a passkey" });
    await waitFor(() => expect(pc.pollPcSetup).toHaveBeenCalledTimes(2), { timeout: 2_000 });
    fireEvent.click(create);
    await waitFor(() => expect(createPasskeyCredential).toHaveBeenCalled());
    refuse(new HttpError(404, "", "", {}));
    await screen.findByText("This didn't work. Start again.");
    credential({});
    expect(await screen.findByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
  });
});

describe("R-71: completed means the passkey exists", () => {
  it("completed with a reason the client does not know is still success", async () => {
    await openAt("completed", "brand_new");
    expect(await screen.findByRole("button", { name: "Sign in with your passkey" })).toBeTruthy();
  });
});

describe("the Linux hint on the web", () => {
  it("is shown on the first passkey screen, before any error", async () => {
    const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0");
    await openAt("enrolled");
    await screen.findByRole("button", { name: "Create a passkey" });
    expect(screen.getByText(/No phone option in Firefox\?/)).toBeTruthy();
    ua.mockRestore();
  });
});
