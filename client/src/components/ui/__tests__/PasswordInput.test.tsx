import { describe, it, expect, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import PasswordInput from "../PasswordInput";

afterEach(() => cleanup());

describe("PasswordInput", () => {
  it("password input toggles its accessible name", () => {
    render(() => <PasswordInput />);
    const toggle = screen.getByRole("button", { name: "Show password" });
    fireEvent.click(toggle);
    expect(screen.getByRole("button", { name: "Hide password" })).toBeTruthy();
  });
});
