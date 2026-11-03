import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent } from "@solidjs/testing-library";
import PasskeyButton from "../PasskeyButton";

afterEach(cleanup);

describe("PasskeyButton", () => {
  it("shows its label and runs onClick", () => {
    const onClick = vi.fn();
    render(() => <PasskeyButton busy={false} onClick={onClick} />);
    const button = screen.getByRole("button", { name: "Sign in with passkey" });
    fireEvent.click(button);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("is busy and disabled while a ceremony runs", () => {
    render(() => <PasskeyButton busy={true} onClick={() => undefined} />);
    const button = screen.getByRole("button");
    expect(button.hasAttribute("disabled")).toBe(true);
    expect(button.getAttribute("aria-busy")).toBe("true");
    expect(button.textContent).toContain("Waiting for your passkey");
  });

  it("has the minimum target size", () => {
    render(() => <PasskeyButton busy={false} onClick={() => undefined} />);
    expect(screen.getByRole("button").className).toContain("min-h-12");
  });
});
