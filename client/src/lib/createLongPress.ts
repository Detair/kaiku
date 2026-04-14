export function createLongPress(
  onLongPress: (x: number, y: number) => void,
  duration = 500
) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let consumed = false;
  let startX = 0;
  let startY = 0;

  const onPointerDown = (e: PointerEvent) => {
    consumed = false;
    startX = e.clientX;
    startY = e.clientY;
    timer = setTimeout(() => {
      consumed = true;
      onLongPress(e.clientX, e.clientY);
      timer = null;
    }, duration);
  };

  const cancel = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  };

  const onPointerMove = (e: PointerEvent) => {
    if (timer && (Math.abs(e.clientX - startX) > 10 || Math.abs(e.clientY - startY) > 10)) {
      cancel();
    }
  };

  const onContextMenu = (e: Event) => {
    // Suppress native context menu only if a long-press was just consumed or
    // is currently pending. Always reset `consumed` afterwards so a subsequent
    // keyboard-triggered context menu (e.g., Shift+F10) is not incorrectly
    // suppressed.
    //
    // Trade-off: on hybrid devices, if the user touch-long-presses then
    // immediately right-clicks before the next pointerdown, the right-click
    // is suppressed. This is acceptable.
    if (timer || consumed) {
      e.preventDefault();
    }
    consumed = false;
  };

  return {
    onPointerDown,
    onPointerUp: cancel,
    onPointerCancel: cancel,
    onPointerMove,
    onContextMenu,
  };
}
