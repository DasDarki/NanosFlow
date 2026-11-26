import { GripVertical, Plus, Search, Trash2 } from "lucide-react";
import { IssueMessage } from "../components/Field";
import type { FieldIssue } from "../types";
import type { NodeCatalog } from "./catalog";
import { LiteralEditor } from "./LiteralEditor";
import type { PinDefinition, TypeExpr, VariableDocument } from "./model";
import { TypePicker } from "./TypePicker";
import { generateUlid } from "./ulid";
import styles from "./VariablesPanel.module.css";

export type PlaceMode = "get" | "set";

interface VariablesPanelProps {
  title: string;
  hint: string;
  catalog: NodeCatalog;
  variables: VariableDocument[];
  issues: FieldIssue[];
  allowSync: boolean;
  onChange: (variables: VariableDocument[]) => void;
  onPlace: (variable: VariableDocument, mode: PlaceMode, event: React.PointerEvent) => void;
  onFindUsages: (variable: VariableDocument) => void;
}

export function parseTypeString(text: string): TypeExpr {
  return text.endsWith("[]") ? { kind: "array", of: { kind: "named", of: text.slice(0, -2) } } : { kind: "named", of: text };
}

function defaultPin(variable: VariableDocument): PinDefinition {
  return {
    name: "default",
    label: "Default",
    type: { kind: "data", type: parseTypeString(variable.type) },
    optional: true,
  };
}

export function VariablesPanel({ title, hint, catalog, variables, issues, allowSync, onChange, onPlace, onFindUsages }: VariablesPanelProps) {
  const update = (id: string, patch: Partial<VariableDocument>) =>
    onChange(variables.map((variable) => (variable.id === id ? { ...variable, ...patch } : variable)));

  const add = () => {
    let index = variables.length + 1;
    while (variables.some((variable) => variable.name.toLowerCase() === `variable${index}`)) {
      index += 1;
    }
    onChange([...variables, { id: generateUlid(), name: `Variable${index}`, type: "number" }]);
  };

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <span>{title}</span>
        <button type="button" className={styles.add} onClick={add}>
          <Plus size={13} /> Add
        </button>
      </div>
      {variables.length === 0 ? (
        <p className={styles.hint}>{hint}</p>
      ) : (
        <p className={styles.hint}>Drag Get or Set onto the canvas, or click to add the node in the middle.</p>
      )}
      {variables.map((variable) => {
        const variableIssues = issues.filter((issue) => String(issue.field) === `variables.${variable.id}`);
        const isArray = variable.type.endsWith("[]");
        return (
          <div key={variable.id} className={styles.variable} data-invalid={variableIssues.length > 0}>
            <div className={styles.row}>
              <GripVertical size={14} className={styles.grip} />
              <input
                className={styles.name}
                value={variable.name}
                aria-label="Variable name"
                spellCheck={false}
                onChange={(event) => update(variable.id, { name: event.target.value })}
              />
              <button type="button" className={styles.place} onPointerDown={(event) => onPlace(variable, "get", event)} title="Drag onto the canvas to read this variable">
                Get
              </button>
              <button type="button" className={styles.place} onPointerDown={(event) => onPlace(variable, "set", event)} title="Drag onto the canvas to change this variable">
                Set
              </button>
              <button type="button" className={styles.remove} title="Find usages of this variable" onClick={() => onFindUsages(variable)}>
                <Search size={13} />
              </button>
              <button
                type="button"
                className={styles.remove}
                title="Delete variable"
                onClick={() => onChange(variables.filter((candidate) => candidate.id !== variable.id))}
              >
                <Trash2 size={13} />
              </button>
            </div>
            <TypePicker
              catalog={catalog}
              value={variable.type}
              label={variable.name}
              onChange={(type) => update(variable.id, { type, default: undefined })}
            />
            {!isArray && (
              <div className={styles.defaultRow}>
                <span>Default</span>
                <LiteralEditor
                  catalog={catalog}
                  pin={defaultPin(variable)}
                  value={variable.default}
                  onChange={(value) => update(variable.id, { default: value })}
                />
              </div>
            )}
            {allowSync && (
              <label className={styles.sync} title="The server sends every change to all clients. Only set it on the server.">
                <input
                  type="checkbox"
                  checked={variable.sync ?? false}
                  onChange={(event) => update(variable.id, { sync: event.target.checked || undefined })}
                />
                Sync to clients
              </label>
            )}
            {variableIssues.map((issue, index) => (
              <IssueMessage key={index} issue={issue} />
            ))}
          </div>
        );
      })}
    </div>
  );
}
