import { beforeEach, describe, expect, it, vi } from "vitest";

const { secureCopy } = vi.hoisted(() => ({
  secureCopy: vi.fn().mockResolvedValue({ success: true }),
}));
vi.mock("@/lib/clipboard", () => ({ secureCopy }));

import { canDismissCreateAccount, copyHandoverText } from "../adminHandover";

describe("adminHandover", () => {
  beforeEach(() => secureCopy.mockClear());

  it("copies through secureCopy with the temporary_password context", async () => {
    await copyHandoverText("secret");
    expect(secureCopy).toHaveBeenCalledWith("secret", { other: "temporary_password" });
  });

  it("propagates secureCopy rejection so the caller can toast", async () => {
    secureCopy.mockRejectedValueOnce(new Error("denied"));
    await expect(copyHandoverText("x")).rejects.toThrow("denied");
  });

  it("does not allow dismissal once the account is created", () => {
    expect(canDismissCreateAccount(false)).toBe(true);
    expect(canDismissCreateAccount(true)).toBe(false);
  });
});
