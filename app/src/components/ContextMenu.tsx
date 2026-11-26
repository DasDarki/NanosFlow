import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import styles from "./ContextMenu.module.css";

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  danger?: boolean;
  separatorBefore?: boolean;
  onSelect: () => void;
}

interface ContextMenuProps {
  position: { x: number; y: number };
  items: MenuItem[];
  onClose: () => void;
}

export function ContextMenu({ position, items, onClose }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [placement, setPlacement] = useState(position);
  const [highlighted, setHighlighted] = useState(0);

  useLayoutEffect(() => {
    const bounds = menuRef.current?.getBoundingClientRect();
    if (bounds) {
      setPlacement({
        x: Math.min(position.x, window.innerWidth - bounds.width - 8),
        y: Math.min(position.y, window.innerHeight - bounds.height - 8),
      });
    }
    menuRef.current?.focus();
  }, [position]);

  useEffect(() => {
    const close = () => onClose();
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
    };
  }, [onClose]);

  return (
    <div
      className={styles.backdrop}
      onMouseDown={onClose}
      onContextMenu={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <div
        ref={menuRef}
        className={styles.menu}
        role="menu"
        tabIndex={-1}
        style={{ left: placement.x, top: placement.y }}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            onClose();
          } else if (event.key === "ArrowDown") {
            event.preventDefault();
            setHighlighted((index) => (index + 1) % items.length);
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setHighlighted((index) => (index - 1 + items.length) % items.length);
          } else if (event.key === "Enter") {
            event.preventDefault();
            onClose();
            items[highlighted]?.onSelect();
          }
        }}
      >
        {items.map((item, index) => (
          <div key={item.label}>
            {item.separatorBefore && <div className={styles.separator} />}
            <button
              type="button"
              role="menuitem"
              className={styles.item}
              data-danger={item.danger}
              data-highlighted={index === highlighted}
              onMouseEnter={() => setHighlighted(index)}
              onClick={() => {
                onClose();
                item.onSelect();
              }}
            >
              <span className={styles.icon}>{item.icon}</span>
              {item.label}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
