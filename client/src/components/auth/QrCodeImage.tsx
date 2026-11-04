/**
 * QrCodeImage — a QR code drawn as SVG, black on white whatever the theme
 * (scanners need the contrast), at least 240 px. The payload is rendered as
 * received; a new payload redraws it.
 */

import { Component, Show, createResource } from "solid-js";

export interface QrCodeImageProps {
  payload: string;
  /** Says what the code is for (never the payload itself). */
  alt: string;
  /** CSS pixels; at least 240. */
  size?: number;
}

const QrCodeImage: Component<QrCodeImageProps> = (props) => {
  const size = () => Math.max(240, props.size ?? 240);
  const [svg] = createResource(
    () => props.payload || null,
    // The encoder loads with the first code (not on the startup path).
    async (payload) => {
      const { default: QRCode } = await import("qrcode");
      return QRCode.toString(payload, { type: "svg", errorCorrectionLevel: "M", margin: 4 });
    },
  );
  return (
    <div
      class="inline-flex items-center justify-center rounded-lg bg-white p-2"
      style={{ width: `${size() + 16}px`, height: `${size() + 16}px` }}
    >
      <Show when={svg()}>
        {(markup) => (
          <img
            src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(markup())}`}
            alt={props.alt}
            width={size()}
            height={size()}
          />
        )}
      </Show>
    </div>
  );
};

export default QrCodeImage;
