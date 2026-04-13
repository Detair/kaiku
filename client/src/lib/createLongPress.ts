export function createLongPress(
  onLongPress: (x: number, y: number) => void,
  duration = 500
) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let startX = 0;
  let startY = 0;

  const onPointerDown = (e: PointerEvent) => {
    startX = e.clientX;
    startY = e.clientY;
    timer = setTimeout(() => {
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
    if (timer) {
      e.preventDefault();
    }
  };

  return {
    onPointerDown,
    onPointerUp: cancel,
    onPointerCancel: cancel,
    onPointerMove,
    onContextMenu,
  };
}
