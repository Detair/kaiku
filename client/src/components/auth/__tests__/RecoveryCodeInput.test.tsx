import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import RecoveryCodeInput, { normalizeRecoveryCode } from "../RecoveryCodeInput";

describe("RecoveryCodeInput", () => {
  it("recovery code input normalises case spaces and dashes", () => {
    expect(normalizeRecoveryCode(" abcde-fghjk mnp ")).toBe("ABCDEFGHJKMNP");
    const [value, setValue] = createSignal("");
    render(() => <RecoveryCodeInput value={value()} onInput={setValue} label="Recovery code" />);
    fireEvent.input(screen.getByLabelText("Recovery code"), { target: { value: "abcde-fghjk mnp" } });
    expect(value()).toBe("ABCDEFGHJKMNP");
    expect((screen.getByLabelText("Recovery code") as HTMLInputElement).value).toBe("ABCDE FGHJK MNP");
  });
});

describe("RecoveryCodeInput separators (Argon review)", () => {
  it("a typed space or dash leaves the field grouped", () => {
    const [value, setValue] = createSignal("ABCDE");
    render(() => <RecoveryCodeInput value={value()} onInput={setValue} label="Code" />);
    const input = screen.getByLabelText("Code") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "ABCDE-" } });
    expect(value()).toBe("ABCDE");
    expect(input.value).toBe("ABCDE");
    fireEvent.input(input, { target: { value: "ABCDE F" } });
    expect(input.value).toBe("ABCDE F");
  });
});
