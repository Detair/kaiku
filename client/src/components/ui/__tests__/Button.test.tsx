import { describe, it, expect, afterEach, vi } from "vitest";
import { createSignal } from "solid-js";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import Button from "../Button";

afterEach(() => cleanup());

describe("Button", () => {
  it("renders an enabled primary button without a hint", () => {
    const onClick = vi.fn();
    render(() => (
      <Button variant="primary" onClick={onClick} disabledReason="Enter a channel name">
        Create Channel
      </Button>
    ));
    const btn = screen.getByRole("button", { name: "Create Channel" }) as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    expect(btn.className).toContain("bg-accent-primary");
    expect(btn.className).toContain("state-disabled");
    expect(screen.queryByText("Enter a channel name")).toBeNull();
    expect(btn.getAttribute("title")).toBeNull();
    fireEvent.click(btn);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("shows why it is disabled as visible text, tooltip and description", () => {
    render(() => (
      <Button variant="primary" disabled disabledReason="Enter a channel name">
        Create Channel
      </Button>
    ));
    const btn = screen.getByRole("button", { name: "Create Channel" }) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    const hint = screen.getByText("Enter a channel name");
    expect(btn.getAttribute("title")).toBe("Enter a channel name");
    expect(btn.getAttribute("aria-describedby")).toBe(hint.id);
    expect(hint.id).not.toBe("");
  });

  it("can keep the reason as a tooltip only", () => {
    render(() => (
      <Button disabled disabledReason="Saving…" showReason={false}>
        Save
      </Button>
    ));
    expect(screen.queryByText("Saving…")).toBeNull();
    expect(screen.getByRole("button").getAttribute("title")).toBe("Saving…");
  });

  it("hint disappears once the button becomes enabled", () => {
    const [name, setName] = createSignal("");
    render(() => (
      <Button disabled={!name()} disabledReason="Enter a channel name">
        Create
      </Button>
    ));
    expect(screen.getByText("Enter a channel name")).toBeTruthy();
    setName("general");
    expect(screen.queryByText("Enter a channel name")).toBeNull();
    expect(screen.getByRole("button").getAttribute("aria-describedby")).toBeNull();
  });

  it("ghost buttons use the quiet disabled style", () => {
    render(() => (
      <Button variant="ghost" disabled>
        Back
      </Button>
    ));
    const cls = screen.getByRole("button").className;
    expect(cls).toContain("state-disabled-quiet");
  });

  it("defaults to type=button so it never submits a form by accident", () => {
    render(() => <Button>Next</Button>);
    expect(screen.getByRole("button").getAttribute("type")).toBe("button");
  });
});
