import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup, screen } from "@solidjs/testing-library";
import QualityTooltip from "../QualityTooltip";
import MicrophoneTest from "../MicrophoneTest";

afterEach(cleanup);

describe("voice text", () => {
  it("quality summary reads as one sentence", () => {
    const { container } = render(() => (
      <QualityTooltip metrics={{ latency: 10, packetLoss: 0, jitter: 1, quality: "good", timestamp: 0 }} />
    ));
    expect(container.textContent).toContain("Quality: Excellent");
  });

  it("microphone test close button has an accessible name", () => {
    // The dialog frame renders in a portal, outside the render container.
    render(() => <MicrophoneTest onClose={() => {}} />);
    expect(screen.getByLabelText("Close")).toBeTruthy();
  });
});
