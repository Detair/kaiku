// NAV-10, NAV-11: "Join a Server" is a translated dialog that closes like the others.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";

vi.mock("@/stores/guilds", () => ({ joinViaInviteCode: vi.fn() }));

import JoinGuildModal from "../JoinGuildModal";

afterEach(cleanup);

describe("JoinGuildModal", () => {
  it("is a dialog that closes with Escape and a click beside it", () => {
    const onClose = vi.fn();
    render(() => <JoinGuildModal onClose={onClose} />);
    const dialog = screen.getByRole("dialog");
    expect(dialog.getAttribute("aria-labelledby")).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent.click(dialog.parentElement!);
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("labels the invite field", () => {
    render(() => <JoinGuildModal onClose={() => {}} />);
    expect(screen.getByLabelText("Invite link or code")).toBeTruthy();
  });
});
