// Components render through @solidjs/testing-library with the real i18n
// (see views/__tests__/Login.test.tsx).
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  createDM: vi.fn(),
  loadDMs: vi.fn(),
  selectDM: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({ createDM: mocks.createDM }));
vi.mock("@/stores/dms", () => ({ loadDMs: mocks.loadDMs, selectDM: mocks.selectDM }));
vi.mock("@/stores/friends", () => ({
  friendsState: {
    friends: [
      { user_id: "u1", username: "anna", display_name: "Anna" },
      { user_id: "u2", username: "ben", display_name: "Ben" },
    ],
  },
  loadFriends: vi.fn(),
}));

import NewMessageModal from "../NewMessageModal";
import { HttpError } from "@/lib/tauri/common";

afterEach(() => {
  cleanup();
  document.body.innerHTML = "";
  vi.clearAllMocks();
});

describe("NewMessageModal", () => {
  it("selected count uses plural forms", () => {
    render(() => <NewMessageModal onClose={vi.fn()} />);
    fireEvent.click(screen.getByText("Anna"));
    expect(screen.getByText("1 user selected")).toBeTruthy();
    fireEvent.click(screen.getByText("Ben"));
    expect(screen.getByText("2 users selected")).toBeTruthy();
    expect(screen.getByText("(Group DM)")).toBeTruthy();
  });

  it("shows the fallback sentence instead of a raw server error", async () => {
    mocks.createDM.mockRejectedValue(new HttpError(400, "HTTP 400: Bad Request"));
    render(() => <NewMessageModal onClose={vi.fn()} />);
    fireEvent.click(screen.getByText("Anna"));
    fireEvent.click(screen.getByTestId("new-dm-submit"));
    expect(await screen.findByText("Failed to create conversation")).toBeTruthy();
    expect(screen.queryByText(/Bad Request/)).toBeNull();
  });
});
