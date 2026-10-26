import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
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
    const { getByLabelText } = render(() => <MicrophoneTest onClose={() => {}} />);
    expect(getByLabelText("Close")).toBeTruthy();
  });
});
