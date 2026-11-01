import { describe, it, expect, afterEach, vi } from "vitest";
import { createSignal } from "solid-js";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import Toggle from "../Toggle";

afterEach(() => cleanup());

describe("Toggle", () => {
  it("is a switch whose aria-checked follows the value", () => {
    const [on, setOn] = createSignal(false);
    render(() => <Toggle checked={on()} onChange={setOn} label="Make server discoverable" />);
    const sw = screen.getByRole("switch", { name: "Make server discoverable" });
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(sw.getAttribute("type")).toBe("button");
    fireEvent.click(sw);
    expect(on()).toBe(true);
    expect(sw.getAttribute("aria-checked")).toBe("true");
  });

  it("calls onChange with the flipped value on click", () => {
    const onChange = vi.fn();
    render(() => <Toggle checked={true} onChange={onChange} label="Threads" />);
    fireEvent.click(screen.getByRole("switch"));
    expect(onChange).toHaveBeenCalledWith(false);
  });

  it("toggles from the keyboard (Space and Enter)", () => {
    const onChange = vi.fn();
    render(() => <Toggle checked={false} onChange={onChange} label="Threads" />);
    const sw = screen.getByRole("switch");
    fireEvent.keyDown(sw, { key: " " });
    fireEvent.keyDown(sw, { key: "Enter" });
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenNthCalledWith(1, true);
  });

  it("shows a check mark only when on, so state is not colour-only", () => {
    const [on, setOn] = createSignal(false);
    render(() => <Toggle checked={on()} onChange={setOn} label="Threads" />);
    const sw = screen.getByRole("switch");
    expect(sw.querySelector("[data-toggle-check]")).toBeNull();
    setOn(true);
    expect(sw.querySelector("[data-toggle-check]")).not.toBeNull();
    expect(sw.dataset.state).toBe("on");
  });

  it("does not change when disabled and exposes the reason", () => {
    const onChange = vi.fn();
    render(() => (
      <Toggle
        checked={false}
        onChange={onChange}
        label="Discoverable"
        disabled
        disabledReason="Discovery is turned off on this server"
      />
    ));
    const sw = screen.getByRole("switch") as HTMLButtonElement;
    expect(sw.disabled).toBe(true);
    fireEvent.click(sw);
    fireEvent.keyDown(sw, { key: " " });
    expect(onChange).not.toHaveBeenCalled();
    expect(sw.getAttribute("title")).toBe("Discovery is turned off on this server");
  });

  it("can be labelled by visible text instead of aria-label", () => {
    render(() => (
      <>
        <span id="lbl">Enable threads</span>
        <Toggle checked={false} onChange={() => {}} labelledBy="lbl" />
      </>
    ));
    expect(screen.getByRole("switch", { name: "Enable threads" })).toBeTruthy();
  });
});
