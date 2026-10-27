import { describe, it, expect, vi } from "vitest";
import { render } from "@solidjs/testing-library";

vi.mock("@floating-ui/dom", () => ({
  computePosition: vi.fn(async () => ({ x: 0, y: 0 })),
  autoUpdate: vi.fn((_ref: unknown, _float: unknown, update: () => void) => {
    update();
    return () => {};
  }),
  flip: vi.fn(() => ({})),
  shift: vi.fn(() => ({})),
  offset: vi.fn(() => ({})),
  size: vi.fn(() => ({})),
}));

import AutocompletePopup from "../AutocompletePopup";

const flush = () => new Promise((r) => setTimeout(r, 5));

describe("AutocompletePopup mentions", () => {
  // The server resolves mentions by username only; inserting the display name
  // ("@Papa") mentioned nobody.
  it("inserts @username, not the display name", async () => {
    const onSelect = vi.fn();
    const anchor = document.createElement("div");
    document.body.appendChild(anchor);
    render(() => (
      <AutocompletePopup
        anchorEl={anchor}
        type="user"
        query="pa"
        selectedIndex={0}
        dmParticipants={[
          { user_id: "u1", username: "thomas", display_name: "Papa", avatar_url: null },
        ]}
        onSelect={onSelect}
        onClose={vi.fn()}
        onSelectionChange={vi.fn()}
      />
    ));
    await flush();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(onSelect).toHaveBeenCalledWith("@thomas ");
  });
});
