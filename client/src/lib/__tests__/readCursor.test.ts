import { describe, expect, it } from "vitest";
import { lastMarkableMessage } from "../readCursor";

const m = (id: string, parent_id: string | null = null) => ({ id, parent_id });

describe("lastMarkableMessage", () => {
  it("skips trailing thread replies", () => {
    expect(lastMarkableMessage([m("a"), m("b"), m("r1", "b")])?.id).toBe("b");
  });

  it("skips optimistic pending messages", () => {
    expect(lastMarkableMessage([m("a"), m("pending:x")])?.id).toBe("a");
  });

  it("returns undefined when only replies or nothing exist", () => {
    expect(lastMarkableMessage([m("r", "p")])).toBeUndefined();
    expect(lastMarkableMessage([])).toBeUndefined();
  });
});
