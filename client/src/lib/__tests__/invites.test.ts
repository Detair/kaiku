import { describe, it, expect } from "vitest";
import { extractInviteCode } from "../invites";

describe("extractInviteCode", () => {
  it("extractInviteCode accepts a code and a link", () => {
    expect(extractInviteCode("AbCd1234")).toBe("AbCd1234");
    expect(extractInviteCode("  AbCd1234  ")).toBe("AbCd1234");
    expect(extractInviteCode("https://kaiku.example/invite/AbCd1234")).toBe("AbCd1234");
    expect(extractInviteCode("https://kaiku.example/invite/AbCd1234/?x=1")).toBe("AbCd1234");
  });

  it("accepts links with a fragment", () => {
    expect(extractInviteCode("https://kaiku.example/invite/AbCd1234#x")).toBe("AbCd1234");
    expect(extractInviteCode("https://kaiku.example/invite/AbCd1234?q=1#x")).toBe("AbCd1234");
    expect(extractInviteCode("https://kaiku.example/invite/AbCd1234/#x")).toBe("AbCd1234");
  });

  it("rejects empty and malformed input", () => {
    expect(extractInviteCode("")).toBeNull();
    expect(extractInviteCode("not a code!")).toBeNull();
    expect(extractInviteCode("https://kaiku.example/other/AbCd1234")).toBeNull();
  });
});
