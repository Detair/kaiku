// Contract amendment 2026-11-03 (C27): new request and registration codes.
import { describe, it, expect } from "vitest";
import { bindErrorText, requestStepError } from "../requestErrors";
import { HttpError } from "@/lib/tauri/common";

describe("request and bind errors", () => {
  it("ACCOUNT_REQUEST_NOT_READY says not yet, or expired", () => {
    expect(requestStepError(new HttpError(409, "x", "ACCOUNT_REQUEST_NOT_READY", { state: "waiting" }), "auth.request.failed")).toBe(
      "This request is not ready yet.",
    );
    expect(requestStepError(new HttpError(409, "x", "ACCOUNT_REQUEST_NOT_READY", { state: "expired" }), "auth.request.failed")).toBe(
      "This request has expired. Start a new one.",
    );
  });

  it("REGISTRATION_TICKET_INVALID says the registration ran out", () => {
    expect(bindErrorText(new HttpError(404, "x", "REGISTRATION_TICKET_INVALID"), "auth.bind.failed")).toBe(
      "This registration ran out or belongs to another device. Register again.",
    );
  });
});
