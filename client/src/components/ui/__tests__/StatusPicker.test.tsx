import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({ updateStatus: vi.fn() }));
vi.mock("@/stores/presence", () => ({ markManualStatusChange: vi.fn() }));
vi.mock("@/stores/focus", () => ({
  focusState: vi.fn(() => ({ auto_activated: false })),
  getActiveFocusMode: vi.fn(() => null),
  deactivateFocusMode: vi.fn(),
}));

import StatusPicker from "../StatusPicker";

afterEach(() => cleanup());

describe("StatusPicker", () => {
  it("offers online, idle and dnd but no invisible option", () => {
    render(() => <StatusPicker currentStatus="online" onClose={() => {}} />);
    expect(screen.getByTestId("status-option-online")).toBeTruthy();
    expect(screen.getByTestId("status-option-idle")).toBeTruthy();
    expect(screen.getByTestId("status-option-dnd")).toBeTruthy();
    expect(screen.queryByTestId("status-option-invisible")).toBeNull();
    expect(screen.queryByText(/invisible/i)).toBeNull();
  });
});
