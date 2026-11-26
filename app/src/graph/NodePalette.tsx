import { Search } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type { ScriptContext } from "../types";
import { formatType, sideRunsIn, type NodeCatalog } from "./catalog";
import type { NodeDefinition, PinType } from "./model";
import { NodeDocs } from "./NodeDocs";
import { definitionColor, pinColor } from "./pinStyle";
import styles from "./NodePalette.module.css";

export interface PaletteFilter {
  pinType: PinType;
  direction: "input" | "output";
  pinLabel: string;
}

interface NodePaletteProps {
  catalog: NodeCatalog;
  context: ScriptContext;
  screenPosition: { x: number; y: number };
  filter?: PaletteFilter;
  onPick: (definition: NodeDefinition) => void;
  onClose: () => void;
}

const RESULT_LIMIT = 150;
const PALETTE_WIDTH = 680;
const PALETTE_HEIGHT = 440;

export function NodePalette({ catalog, context, screenPosition, filter, onPick, onClose }: NodePaletteProps) {
  const [query, setQuery] = useState("");
  const [highlighted, setHighlighted] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const { results, total } = useMemo(() => {
    const matches = catalog.search(query, filter);
    const available = matches.filter((definition) => sideRunsIn(definition.side, context));
    const unavailable = matches.filter((definition) => !sideRunsIn(definition.side, context));
    const ordered = [...available, ...unavailable];
    return { results: ordered.slice(0, RESULT_LIMIT), total: ordered.length };
  }, [catalog, context, filter, query]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    setHighlighted(0);
  }, [query]);

  useLayoutEffect(() => {
    const element = listRef.current?.querySelector<HTMLElement>(`[data-index="${highlighted}"]`);
    element?.scrollIntoView({ block: "nearest" });
  }, [highlighted]);

  const left = Math.max(12, Math.min(screenPosition.x, window.innerWidth - PALETTE_WIDTH - 12));
  const top = Math.max(12, Math.min(screenPosition.y, window.innerHeight - PALETTE_HEIGHT - 12));
  const current = results[highlighted];

  return (
    <div className={styles.backdrop} onMouseDown={onClose} onContextMenu={(event) => event.preventDefault()}>
      <div
        className={styles.palette}
        style={{ left, top, width: PALETTE_WIDTH, height: PALETTE_HEIGHT }}
        role="dialog"
        aria-label="Add node"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className={styles.listPane}>
          <div className={styles.searchRow}>
            <Search size={15} />
            <input
              ref={inputRef}
              className={styles.search}
              value={query}
              placeholder={filter ? "Search compatible nodes" : `Search ${catalog.size} nodes`}
              spellCheck={false}
              aria-label="Search nodes"
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "ArrowDown") {
                  event.preventDefault();
                  setHighlighted((index) => Math.min(index + 1, results.length - 1));
                } else if (event.key === "ArrowUp") {
                  event.preventDefault();
                  setHighlighted((index) => Math.max(index - 1, 0));
                } else if (event.key === "Enter" && current) {
                  event.preventDefault();
                  onPick(current);
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  onClose();
                }
              }}
            />
          </div>
          {filter && (
            <div className={styles.filter}>
              <span className={styles.filterDot} style={{ "--pin": pinColor(catalog, filter.pinType) } as CSSProperties} />
              {filter.direction === "input" ? "Takes" : "Gives"}{" "}
              <code>{filter.pinType.kind === "exec" ? "execution" : formatType(filter.pinType.type)}</code>
              <span className={styles.filterSource}>from {filter.pinLabel}</span>
            </div>
          )}
          <div className={styles.results} ref={listRef} role="listbox" aria-label="Nodes">
            {results.length === 0 && (
              <div className={styles.empty}>
                No node matches "{query}".
                {filter && " Only nodes that fit the dragged pin are shown."}
              </div>
            )}
            {results.map((definition, index) => {
              const available = sideRunsIn(definition.side, context);
              return (
                <button
                  key={definition.id}
                  type="button"
                  role="option"
                  data-index={index}
                  aria-selected={index === highlighted}
                  className={styles.result}
                  data-available={available}
                  onMouseMove={() => setHighlighted(index)}
                  onClick={() => onPick(definition)}
                >
                  <span className={styles.kindDot} style={{ "--kind": definitionColor(definition) } as CSSProperties} />
                  <span className={styles.resultTitle}>{definition.title}</span>
                  {!available && <span className={styles.sideBadge}>{definition.side} only</span>}
                  <span className={styles.category}>{definition.subtitle ?? definition.category}</span>
                </button>
              );
            })}
          </div>
          <div className={styles.footer}>
            <span>
              {total > RESULT_LIMIT ? `${RESULT_LIMIT} of ${total}` : total} results
            </span>
            <span>
              <kbd>↑</kbd>
              <kbd>↓</kbd> select <kbd>Enter</kbd> add <kbd>Esc</kbd> close
            </span>
          </div>
        </div>
        <div className={styles.preview}>
          {current ? (
            <NodeDocs definition={current} catalog={catalog} context={context} />
          ) : (
            <p className={styles.previewEmpty}>Pick a node to see what it does.</p>
          )}
        </div>
      </div>
    </div>
  );
}
