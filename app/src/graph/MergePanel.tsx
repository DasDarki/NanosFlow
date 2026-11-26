import { GitMerge, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "../components/Button";
import type { ConflictChoice, ConflictSide, FileConflict, PinValue } from "./model";
import styles from "./MergePanel.module.css";

interface MergePanelProps {
  conflicts: FileConflict[];
  nodeTitle: (nodeType: string) => string;
  linkedTitle: (nodeId: string) => string;
  busy: boolean;
  onResolve: (conflict: FileConflict, choice: ConflictChoice) => void;
  onResolveAll: (choice: ConflictChoice) => void;
  onClose: () => void;
}

type Rows = Map<string, string>;

function text(value: unknown): string {
  if (value === undefined || value === null) {
    return "";
  }
  return typeof value === "string" ? value : JSON.stringify(value);
}

function pinValue(value: PinValue, linkedTitle: (nodeId: string) => string): string {
  return "literal" in value ? text(value.literal) : `from ${linkedTitle(value.node)}, ${value.pin}`;
}

function rowsOf(side: ConflictSide, nodeTitle: (nodeType: string) => string, linkedTitle: (nodeId: string) => string): Rows {
  const rows: Rows = new Map();
  switch (side.type) {
    case "node": {
      const { node } = side;
      rows.set("Node", nodeTitle(node.type));
      rows.set("Position", `${Math.round(node.position[0])}, ${Math.round(node.position[1])}`);
      for (const [pin, value] of Object.entries(node.inputs ?? {})) {
        rows.set(`Input ${pin}`, pinValue(value, linkedTitle));
      }
      for (const [pin, link] of Object.entries(node.exec ?? {})) {
        rows.set(`Runs ${pin}`, `${linkedTitle(link.node)}, ${link.pin}`);
      }
      for (const [key, value] of Object.entries(node.properties ?? {})) {
        rows.set(`Setting ${key}`, text(value));
      }
      return rows;
    }
    case "variable": {
      const { variable } = side;
      rows.set("Name", variable.name);
      rows.set("Type", variable.type);
      rows.set("Default", text(variable.default));
      rows.set("Synced", variable.sync ? "yes" : "no");
      rows.set("Description", variable.description ?? "");
      return rows;
    }
    case "graph": {
      const { manifest } = side;
      rows.set("Kind", manifest.kind);
      rows.set("Description", manifest.description ?? "");
      if (manifest.parent) {
        rows.set("Parent", manifest.parent);
      }
      if (manifest.direction) {
        rows.set("Direction", manifest.direction);
      }
      rows.set("Pure", manifest.pure ? "yes" : "no");
      rows.set("Exported", manifest.exported ? "yes" : "no");
      for (const [list, pins] of [
        ["Parameter", manifest.parameters ?? []],
        ["Return", manifest.returns ?? []],
      ] as const) {
        pins.forEach((pin) => rows.set(`${list} ${pin.id}`, `${pin.name}: ${pin.type}`));
      }
      (manifest.interfaces ?? []).forEach((name) => rows.set(`Implements ${name}`, "yes"));
      (manifest.functions ?? []).forEach((member) => rows.set(`Function ${member.id}`, member.name));
      return rows;
    }
    case "deleted":
      rows.set("File", "Deleted in this version");
      return rows;
    case "unreadable":
      rows.set("Problem", side.error);
      rows.set("Text", side.text);
      return rows;
  }
}

function conflictLabel(conflict: FileConflict, nodeTitle: (nodeType: string) => string): string {
  const side = conflict.ours.type !== "deleted" ? conflict.ours : conflict.theirs;
  switch (side.type) {
    case "node":
      return nodeTitle(side.node.type);
    case "variable":
      return `Variable ${side.variable.name}`;
    case "graph":
      return "Graph settings";
    default:
      return conflict.file;
  }
}

export function MergePanel({ conflicts, nodeTitle, linkedTitle, busy, onResolve, onResolveAll, onClose }: MergePanelProps) {
  const [selected, setSelected] = useState(0);
  const index = Math.min(selected, Math.max(conflicts.length - 1, 0));
  const conflict = conflicts[index];

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const ours = conflict ? rowsOf(conflict.ours, nodeTitle, linkedTitle) : new Map<string, string>();
  const theirs = conflict ? rowsOf(conflict.theirs, nodeTitle, linkedTitle) : new Map<string, string>();
  const keys = [...new Set([...ours.keys(), ...theirs.keys()])];
  const oursLabel = conflict?.oursLabel || "Yours";
  const theirsLabel = conflict?.theirsLabel || "Theirs";

  return (
    <div className={styles.backdrop} onMouseDown={onClose}>
      <div className={styles.dialog} role="dialog" aria-label="Resolve merge conflicts" onMouseDown={(event) => event.stopPropagation()}>
        <header className={styles.header}>
          <GitMerge size={18} className={styles.icon} />
          <div className={styles.titles}>
            <h2>Resolve merge conflicts</h2>
            <p>
              {conflicts.length === 0
                ? "Everything is resolved."
                : `${conflicts.length} ${conflicts.length === 1 ? "entry was" : "entries were"} changed on both sides. Pick the version to keep for each one.`}
            </p>
          </div>
          {conflicts.length > 1 && (
            <>
              <Button variant="ghost" size="small" disabled={busy} onClick={() => onResolveAll("ours")}>
                Keep all yours
              </Button>
              <Button variant="ghost" size="small" disabled={busy} onClick={() => onResolveAll("theirs")}>
                Take all theirs
              </Button>
            </>
          )}
          <button type="button" className={styles.close} onClick={onClose} aria-label="Close">
            <X size={15} />
          </button>
        </header>
        {conflict ? (
          <div className={styles.body}>
            <ul className={styles.list}>
              {conflicts.map((entry, position) => (
                <li key={entry.file}>
                  <button type="button" aria-current={position === index} onClick={() => setSelected(position)}>
                    <span>{conflictLabel(entry, nodeTitle)}</span>
                    <code>{entry.file}</code>
                  </button>
                </li>
              ))}
            </ul>
            <div className={styles.compare}>
              <div className={styles.table} role="table">
                <div className={styles.headRow} role="row">
                  <span role="columnheader" />
                  <span role="columnheader">Yours ({oursLabel})</span>
                  <span role="columnheader">Theirs ({theirsLabel})</span>
                </div>
                {keys.map((key) => {
                  const mine = ours.get(key);
                  const other = theirs.get(key);
                  return (
                    <div key={key} className={styles.row} role="row" data-differs={mine !== other}>
                      <span role="rowheader">{key}</span>
                      <span role="cell" data-missing={mine === undefined}>
                        {mine ?? "not set"}
                      </span>
                      <span role="cell" data-missing={other === undefined}>
                        {other ?? "not set"}
                      </span>
                    </div>
                  );
                })}
              </div>
              <div className={styles.actions}>
                <Button variant="secondary" size="small" disabled={busy} onClick={() => onResolve(conflict, "ours")}>
                  Keep yours
                </Button>
                <Button variant="primary" size="small" disabled={busy} onClick={() => onResolve(conflict, "theirs")}>
                  Take theirs
                </Button>
              </div>
            </div>
          </div>
        ) : (
          <div className={styles.done}>
            <p>All conflicts of this graph are resolved. Commit the result in your version control tool.</p>
            <Button variant="primary" size="small" onClick={onClose}>
              Done
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}
