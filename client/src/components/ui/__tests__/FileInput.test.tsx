import { describe, it, expect, afterEach, vi } from "vitest";
import { createSignal } from "solid-js";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import FileInput from "../FileInput";

afterEach(() => cleanup());

describe("FileInput", () => {
  it("shows a themed button and 'No file chosen' instead of the native control", () => {
    render(() => <FileInput onFile={() => {}} />);
    expect(screen.getByRole("button", { name: "Choose file" })).toBeTruthy();
    expect(screen.getByText("No file chosen")).toBeTruthy();
  });

  it("opens the native picker from the button", () => {
    render(() => <FileInput onFile={() => {}} data-testid="fi" />);
    const native = screen.getByTestId("fi") as HTMLInputElement;
    const click = vi.spyOn(native, "click");
    fireEvent.click(screen.getByRole("button", { name: "Choose file" }));
    expect(click).toHaveBeenCalledOnce();
  });

  it("reports the chosen file and shows its name", () => {
    const [name, setName] = createSignal<string | null>(null);
    const onFile = vi.fn((f: File) => setName(f.name));
    render(() => <FileInput onFile={onFile} fileName={name()} data-testid="fi" />);
    const native = screen.getByTestId("fi") as HTMLInputElement;
    const file = new File(["x"], "banner.png", { type: "image/png" });
    Object.defineProperty(native, "files", { value: [file], configurable: true });
    fireEvent.change(native);
    expect(onFile).toHaveBeenCalledWith(file);
    expect(screen.getByText("banner.png")).toBeTruthy();
  });

  it("disables both the button and the native input", () => {
    render(() => <FileInput onFile={() => {}} disabled data-testid="fi" />);
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("fi") as HTMLInputElement).disabled).toBe(true);
  });
});
