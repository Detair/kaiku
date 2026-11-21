import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";

vi.mock("../MicTestPanel", () => ({ default: () => <p>panel</p> }));

import MicrophoneTest from "../MicrophoneTest";

afterEach(cleanup);

describe("MicrophoneTest", () => {
  it("is a dialog that Escape closes", () => {
    const onClose = vi.fn();
    render(() => <MicrophoneTest onClose={onClose} />);
    const dialog = screen.getByRole("dialog");
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
