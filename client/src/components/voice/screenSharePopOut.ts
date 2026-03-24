/** Manages pop-out windows for screen share streams. */

const popOutWindows = new Map<string, Window>();

/** Open a screen share in a new browser window. */
export function popOut(
  streamId: string,
  track: MediaStreamTrack,
  label: string,
  onClose: () => void,
): void {
  bringBack(streamId);

  const win = window.open("", "_blank", "width=960,height=540,menubar=no,toolbar=no");
  if (!win) {
    console.warn("[PopOut] Popup blocked by browser");
    return;
  }

  popOutWindows.set(streamId, win);

  // Build the pop-out page once the new window's document is ready.
  // window.open("") gives us about:blank which may not be fully initialized.
  const buildPage = () => {
    const doc = win.document;

    // Clear any default content and set up the document
    doc.open();
    doc.close();
    doc.title = label;

    const style = doc.createElement("style");
    style.textContent = `
      * { margin: 0; padding: 0; box-sizing: border-box; }
      body { background: #0d0d1a; display: flex; align-items: center; justify-content: center; height: 100vh; overflow: hidden; }
      video { max-width: 100%; max-height: 100%; object-fit: contain; }
      .label { position: fixed; bottom: 12px; left: 12px; color: #ccc; font-family: system-ui; font-size: 13px; background: rgba(0,0,0,0.6); padding: 4px 12px; border-radius: 6px; }
    `;
    doc.head.appendChild(style);

    const video = doc.createElement("video");
    video.autoplay = true;
    video.playsInline = true;
    video.muted = true; // Muted allows autoplay without user gesture in pop-out window
    video.srcObject = new MediaStream([track]);
    doc.body.appendChild(video);

    const labelEl = doc.createElement("div");
    labelEl.className = "label";
    labelEl.textContent = label;
    doc.body.appendChild(labelEl);

    // Try to play explicitly in case autoplay is still blocked
    video.play().catch(() => {
      // Autoplay blocked — user can click to play
      video.controls = true;
    });

    win.addEventListener("beforeunload", () => {
      video.srcObject = null;
      popOutWindows.delete(streamId);
      onClose();
    });
  };

  // Use requestAnimationFrame to ensure the window's document is ready
  win.requestAnimationFrame(buildPage);
}

/** Bring a popped-out stream back to inline. */
export function bringBack(streamId: string): void {
  const win = popOutWindows.get(streamId);
  if (win && !win.closed) win.close();
  popOutWindows.delete(streamId);
}

/** Check if a stream is currently popped out. */
export function isPoppedOut(streamId: string): boolean {
  const win = popOutWindows.get(streamId);
  return !!win && !win.closed;
}

/** Close all pop-out windows. */
export function closeAll(): void {
  for (const [, win] of popOutWindows) {
    if (!win.closed) win.close();
  }
  popOutWindows.clear();
}
