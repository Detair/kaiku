import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import CodeEntry, { formatCode } from "../CodeEntry";

function mount() {
  const submit = vi.fn();
  const [value, setValue] = createSignal("");
  render(() => <CodeEntry length={6} value={value()} onInput={setValue} onSubmit={submit} label="Code from the phone" />);
  return { submit, value };
}

describe("CodeEntry", () => {
  it("formats six digits in groups", () => {
    expect(formatCode("123456")).toBe("123 456");
    expect(formatCode("1234")).toBe("123 4");
    expect(formatCode("")).toBe("");
  });

  it("code entry never submits by itself", () => {
    const { submit } = mount();
    const input = screen.getByLabelText("Code from the phone") as HTMLInputElement;
    fireEvent.input(input, { target: { value: "123456" } });
    expect(submit).not.toHaveBeenCalled();
    const confirm = screen.getByRole("button", { name: "Confirm" }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(false);
    fireEvent.click(confirm);
    expect(submit).toHaveBeenCalledTimes(1);
  });

  it("code entry groups digits and allows deleting", () => {
    const { value } = mount();
    const input = screen.getByLabelText("Code from the phone") as HTMLInputElement;
    expect(input.getAttribute("inputmode")).toBe("numeric");
    expect(input.getAttribute("autocomplete")).toBe("one-time-code");
    fireEvent.input(input, { target: { value: " 12 3a45" } });
    expect(value()).toBe("12345");
    expect(input.value).toBe("123 45");
    expect((screen.getByRole("button", { name: "Confirm" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(value()).toBe("1234");
    fireEvent.click(screen.getByRole("button", { name: "Digit 9" }));
    fireEvent.click(screen.getByRole("button", { name: "Digit 8" }));
    expect(value()).toBe("123498");
    fireEvent.click(screen.getByRole("button", { name: "Digit 7" }));
    expect(value()).toBe("123498");
    expect(input.value).toBe("123 498");
  });
});
