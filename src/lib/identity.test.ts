import { describe, expect, it } from "vitest";
import { shouldShowIdentityNudge } from "./identity";

describe("shouldShowIdentityNudge", () => {
  it("shows the nudge once a folder is watched and identity is unset", () => {
    expect(shouldShowIdentityNudge({ name: "", email: "" }, 1)).toBe(true);
  });

  it("skips the nudge when nothing is being watched", () => {
    expect(shouldShowIdentityNudge({ name: "", email: "" }, 0)).toBe(false);
    expect(shouldShowIdentityNudge({ name: "", email: "" }, null)).toBe(false);
  });

  it("skips the nudge once a name or email is set", () => {
    expect(shouldShowIdentityNudge({ name: "Eric", email: "" }, 1)).toBe(false);
    expect(shouldShowIdentityNudge({ name: "", email: "eric@example.com" }, 1)).toBe(false);
  });
});
