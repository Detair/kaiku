// The client's step-up action table (C26–C29 common rules) must stay the
// server's `StepUpAction::ALL`: the same strings, in the same order. LP's
// passkey module and every step-up use this one table.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { STEP_UP_ACTIONS } from "../login";

const STEP_UP_RS = resolve(__dirname, "../../../../../server/src/auth/step_up.rs");

/** The server's actions in `ALL` order, read from the source. */
function serverActions(): string[] {
  const src = readFileSync(STEP_UP_RS, "utf8");
  const all = /pub const ALL: \[Self; (\d+)\] = \[([\s\S]*?)\];/.exec(src);
  const asStr = /pub const fn as_str\(self\) -> &'static str \{\s*match self \{([\s\S]*?)\n\s*\}\n\s*\}/.exec(src);
  if (!all || !asStr) throw new Error("StepUpAction::ALL / as_str not found in step_up.rs");
  const names = [...all[2].matchAll(/Self::(\w+)/g)].map((m) => m[1]);
  const wire = new Map([...asStr[1].matchAll(/Self::(\w+) => "([^"]+)"/g)].map((m) => [m[1], m[2]]));
  expect(names).toHaveLength(Number(all[1]));
  return names.map((n) => {
    const w = wire.get(n);
    if (!w) throw new Error(`no wire string for ${n}`);
    return w;
  });
}

describe("STEP_UP_ACTIONS", () => {
  it("is the server's StepUpAction::ALL", () => {
    expect([...STEP_UP_ACTIONS]).toEqual(serverActions());
  });

  it("names every admin action with the admin. prefix (NOT_ADMIN for others)", () => {
    const admin = STEP_UP_ACTIONS.filter((a) => a.startsWith("admin."));
    expect(admin).toEqual([
      "admin.elevate",
      "admin.enrollment.issue",
      "admin.authenticator.remove",
      "admin.may_approve",
      "admin.factor.set_state",
      "admin.recovery_contact.set",
    ]);
  });
});
