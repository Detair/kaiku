import { describe, expect, it } from "vitest";
import { addAvailableTrack, removeTracksOfUser, viewerState } from "../screenShareViewer";

describe("own self-preview at the end of a session", () => {
  it("removes only that user's tracks", () => {
    addAvailableTrack("own", null, "me", "Me", "Screen");
    addAvailableTrack("other", null, "u2", "Bob", "Screen");
    removeTracksOfUser("me");
    expect([...viewerState.availableTracks.keys()]).toEqual(["other"]);
    removeTracksOfUser("u2");
  });
});
