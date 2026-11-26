import { ChevronRight, Search } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type { NodeListLayout, ScriptContext } from "../types";
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
  layout?: NodeListLayout;
  onPick: (definition: NodeDefinition) => void;
  onClose: () => void;
}

type Row =
  | { type: "group"; key: string; title: string; count: number }
  | { type: "category"; key: string; title: string; count: number; expanded: boolean }
  | { type: "node"; key: string; definition: NodeDefinition; nested: boolean };

const RESULT_LIMIT = 150;
const PALETTE_WIDTH = 680;
const PALETTE_HEIGHT = 440;
const AUTO_EXPAND_LIMIT = 40;
const EXPANDED_STORAGE = "nanosflow.palette.expanded";

function readExpanded(): Set<string> {
  try {
    return new Set(JSON.parse(window.localStorage.getItem(EXPANDED_STORAGE) ?? "[]") as string[]);
  } catch {
    return new Set();
  }
}

function writeExpanded(expanded: Set<string>) {
  try {
    window.localStorage.setItem(EXPANDED_STORAGE, JSON.stringify([...expanded]));
  } catch {
    return;
  }
}

export function NodePalette({ catalog, context, screenPosition, filter, layout = "flat", onPick, onClose }: NodePaletteProps) {
  const [query, setQuery] = useState("");
  const [highlighted, setHighlighted] = useState(0);
  const [expanded, setExpanded] = useState<Set<string>>(readExpanded);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const browsing = layout === "categorized" && query.trim() === "";

  const flat = useMemo(() => {
    if (browsing) {
      return { results: [] as NodeDefinition[], total: 0 };
    }
    const matches = catalog.search(query, filter);
    const available = matches.filter((definition) => sideRunsIn(definition.side, context));
    const unavailable = matches.filter((definition) => !sideRunsIn(definition.side, context));
    const ordered = [...available, ...unavailable];
    return { results: ordered.slice(0, RESULT_LIMIT), total: ordered.length };
  }, [browsing, catalog, context, filter, query]);

  const groups = useMemo(() => (browsing ? catalog.browse(context, filter) : []), [browsing, catalog, context, filter]);
  const browseTotal = groups.reduce((total, group) => total + group.count, 0);
  const expandAll = browseTotal <= AUTO_EXPAND_LIMIT;

  const rows = useMemo<Row[]>(() => {
    if (!browsing) {
      return flat.results.map((definition) => ({ type: "node", key: definition.id, definition, nested: false }));
    }
    const list: Row[] = [];
    for (const group of groups) {
      list.push({ type: "group", key: `group:${group.title}`, title: group.title, count: group.count });
      for (const category of group.categories) {
        const key = `${group.title}/${category.title}`;
        const open = expandAll || expanded.has(key);
        list.push({ type: "category", key, title: category.title, count: category.definitions.length, expanded: open });
        if (open) {
          for (const definition of category.definitions) {
            list.push({ type: "node", key: `${key}/${definition.id}`, definition, nested: true });
          }
        }
      }
    }
    return list;
  }, [browsing, flat.results, groups, expanded, expandAll]);

  const selectable = useMemo(() => rows.map((row, index) => (row.type === "group" ? -1 : index)).filter((index) => index >= 0), [rows]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    setHighlighted(selectable[0] ?? 0);
  }, [query, layout]);

  useLayoutEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-index="${highlighted}"]`)?.scrollIntoView({ block: "nearest" });
  }, [highlighted]);

  const toggle = (key: string, open?: boolean) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (open ?? !next.has(key)) {
        next.add(key);
      } else {
        next.delete(key);
      }
      writeExpanded(next);
      return next;
    });
  };

  const move = (step: number) => {
    const position = selectable.indexOf(highlighted);
    const next = selectable[Math.min(Math.max(position + step, 0), selectable.length - 1)];
    if (next !== undefined) {
      setHighlighted(next);
    }
  };

  const activate = (row: Row | undefined) => {
    if (!row) {
      return;
    }
    if (row.type === "node") {
      onPick(row.definition);
    } else if (row.type === "category") {
      toggle(row.key);
    }
  };

  const left = Math.max(12, Math.min(screenPosition.x, window.innerWidth - PALETTE_WIDTH - 12));
  const top = Math.max(12, Math.min(screenPosition.y, window.innerHeight - PALETTE_HEIGHT - 12));
  const currentRow = rows[highlighted];
  const current = currentRow?.type === "node" ? currentRow.definition : undefined;
  const nodeCount = browsing ? browseTotal : flat.total;

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
                  move(1);
                } else if (event.key === "ArrowUp") {
                  event.preventDefault();
                  move(-1);
                } else if (event.key === "ArrowRight" && currentRow?.type === "category" && !currentRow.expanded) {
                  event.preventDefault();
                  toggle(currentRow.key, true);
                } else if (event.key === "ArrowLeft" && currentRow?.type === "category" && currentRow.expanded && !expandAll) {
                  event.preventDefault();
                  toggle(currentRow.key, false);
                } else if (event.key === "Enter") {
                  event.preventDefault();
                  activate(currentRow);
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
            {rows.length === 0 && (
              <div className={styles.empty}>
                No node matches "{query}".
                {filter && " Only nodes that fit the dragged pin are shown."}
              </div>
            )}
            {rows.map((row, index) => {
              if (row.type === "group") {
                return (
                  <div key={row.key} className={styles.groupTitle}>
                    {row.title}
                    <span>{row.count}</span>
                  </div>
                );
              }
              if (row.type === "category") {
                return (
                  <button
                    key={row.key}
                    type="button"
                    role="option"
                    data-index={index}
                    aria-selected={index === highlighted}
                    aria-expanded={row.expanded}
                    className={styles.categoryRow}
                    tabIndex={-1}
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseMove={() => setHighlighted(index)}
                    onClick={() => toggle(row.key)}
                  >
                    <ChevronRight size={13} className={styles.chevron} />
                    <span className={styles.resultTitle}>{row.title}</span>
                    <span className={styles.count}>{row.count}</span>
                  </button>
                );
              }
              const definition = row.definition;
              const available = sideRunsIn(definition.side, context);
              return (
                <button
                  key={row.key}
                  type="button"
                  role="option"
                  data-index={index}
                  data-nested={row.nested}
                  aria-selected={index === highlighted}
                  className={styles.result}
                  data-available={available}
                  tabIndex={-1}
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseMove={() => setHighlighted(index)}
                  onClick={() => onPick(definition)}
                >
                  <span className={styles.kindDot} style={{ "--kind": definitionColor(definition) } as CSSProperties} />
                  <span className={styles.resultTitle}>{definition.title}</span>
                  {!available && <span className={styles.sideBadge}>{definition.side} only</span>}
                  {!row.nested && <span className={styles.category}>{definition.subtitle ?? definition.category}</span>}
                </button>
              );
            })}
          </div>
          <div className={styles.footer}>
            <span>
              {!browsing && flat.total > RESULT_LIMIT ? `${RESULT_LIMIT} of ${flat.total}` : nodeCount} {browsing ? "nodes" : "results"}
            </span>
            <span>
              <kbd>↑</kbd>
              <kbd>↓</kbd> select {browsing && <><kbd>→</kbd> open </>}<kbd>Enter</kbd> add <kbd>Esc</kbd> close
            </span>
          </div>
        </div>
        <div className={styles.preview}>
          {current ? (
            <NodeDocs definition={current} catalog={catalog} context={context} />
          ) : (
            <p className={styles.previewEmpty}>
              {browsing ? "Open a category to browse its nodes, or type to search everything." : "Pick a node to see what it does."}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
