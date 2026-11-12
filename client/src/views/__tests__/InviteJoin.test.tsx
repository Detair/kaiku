// NAV-2: after joining through an invite link the app opens the server.
import { describe, expect, it, vi, afterEach } from "vitest";
import { cleanup, render } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  navigate: vi.fn(),
  joinViaInviteCode: vi.fn(),
}));

vi.mock("@solidjs/router", () => ({
  useParams: () => ({ code: "abc" }),
  useNavigate: () => mocks.navigate,
}));
vi.mock("@/stores/guilds", () => ({ joinViaInviteCode: mocks.joinViaInviteCode }));
vi.mock("@/stores/auth", () => ({ authState: { user: { id: "me" } } }));

import InviteJoin from "../InviteJoin";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("InviteJoin", () => {
  it("goes to the app once the server is joined", async () => {
    mocks.joinViaInviteCode.mockResolvedValue(undefined);
    render(() => <InviteJoin />);
    await vi.waitFor(() =>
      expect(mocks.navigate).toHaveBeenCalledWith("/", { replace: true }),
    );
    expect(mocks.joinViaInviteCode).toHaveBeenCalledWith("abc");
  });

  it("stays on the page with the reason when joining fails", async () => {
    mocks.joinViaInviteCode.mockRejectedValue(new Error("nope"));
    const { findByText } = render(() => <InviteJoin />);
    expect(await findByText("Go Home")).toBeTruthy();
    expect(mocks.navigate).not.toHaveBeenCalled();
  });
});
