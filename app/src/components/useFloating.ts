import { useCallback, useEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";

interface Position {
  x: number;
  y: number;
}

const STORAGE_PREFIX = "nanosflow.panel.";
const VISIBLE_MARGIN = 80;
const INTERACTIVE = "button, input, select, textarea, a, label, [role='switch'], [data-no-drag]";

function readPosition(id: string): Position | null {
  try {
    const stored = window.localStorage.getItem(STORAGE_PREFIX + id);
    const parsed = stored ? (JSON.parse(stored) as Position) : null;
    return parsed && Number.isFinite(parsed.x) && Number.isFinite(parsed.y) ? parsed : null;
  } catch {
    return null;
  }
}

function writePosition(id: string, position: Position | null) {
  try {
    if (position) {
      window.localStorage.setItem(STORAGE_PREFIX + id, JSON.stringify(position));
    } else {
      window.localStorage.removeItem(STORAGE_PREFIX + id);
    }
  } catch {
    return;
  }
}

function clamp(position: Position, element: HTMLElement): Position {
  const parent = (element.offsetParent as HTMLElement | null) ?? document.body;
  const bounds = parent.getBoundingClientRect();
  const width = element.offsetWidth;
  return {
    x: Math.min(Math.max(position.x, VISIBLE_MARGIN - width), bounds.width - VISIBLE_MARGIN),
    y: Math.min(Math.max(position.y, 0), bounds.height - 40),
  };
}

export interface Floating<T extends HTMLElement> {
  ref: React.RefObject<T | null>;
  style: CSSProperties | undefined;
  handle: {
    onPointerDown: (event: ReactPointerEvent) => void;
    onDoubleClick: (event: React.MouseEvent) => void;
    "data-drag-handle": true;
  };
  moved: boolean;
}

export function useFloating<T extends HTMLElement>(id: string, existing?: React.RefObject<T | null>): Floating<T> {
  const own = useRef<T>(null);
  const ref = existing ?? own;
  const [position, setPosition] = useState<Position | null>(() => readPosition(id));

  useEffect(() => {
    const element = ref.current;
    if (!position || !element) {
      return;
    }
    const onResize = () => setPosition((current) => (current ? clamp(current, element) : current));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [position]);

  const onPointerDown = useCallback(
    (event: ReactPointerEvent) => {
      const element = ref.current;
      if (event.button !== 0 || !element || (event.target as HTMLElement).closest(INTERACTIVE)) {
        return;
      }
      event.preventDefault();
      const parent = (element.offsetParent as HTMLElement | null) ?? document.body;
      const parentBounds = parent.getBoundingClientRect();
      const bounds = element.getBoundingClientRect();
      const grab = { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
      let latest: Position = { x: bounds.left - parentBounds.left, y: bounds.top - parentBounds.top };

      const move = (moveEvent: PointerEvent) => {
        latest = clamp({ x: moveEvent.clientX - parentBounds.left - grab.x, y: moveEvent.clientY - parentBounds.top - grab.y }, element);
        setPosition(latest);
      };
      const stop = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", stop);
        document.body.style.removeProperty("user-select");
        writePosition(id, latest);
      };
      document.body.style.setProperty("user-select", "none");
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", stop);
    },
    [id],
  );

  const onDoubleClick = useCallback(
    (event: React.MouseEvent) => {
      if ((event.target as HTMLElement).closest(INTERACTIVE)) {
        return;
      }
      setPosition(null);
      writePosition(id, null);
    },
    [id],
  );

  return {
    ref,
    style: position ? { left: position.x, top: position.y, right: "auto", bottom: "auto", transform: "none" } : undefined,
    handle: { onPointerDown, onDoubleClick, "data-drag-handle": true },
    moved: position !== null,
  };
}
