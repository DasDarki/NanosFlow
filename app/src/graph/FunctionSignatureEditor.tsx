import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import { IssueMessage } from "../components/Field";
import type { FieldIssue } from "../types";
import type { NodeCatalog } from "./catalog";
import type { FunctionPin, FunctionSignature } from "./model";
import { TypePicker } from "./TypePicker";
import { generateUlid } from "./ulid";
import styles from "./FunctionSignatureEditor.module.css";

export type SignatureVariant = "function" | "method" | "macro";

interface FunctionSignatureEditorProps {
  variant: SignatureVariant;
  libraryName: string;
  name: string;
  catalog: NodeCatalog;
  signature: FunctionSignature;
  issues: FieldIssue[];
  onChange: (signature: FunctionSignature) => void;
}

interface PinListProps {
  title: string;
  singular: string;
  field: "parameters" | "returns";
  pins: FunctionPin[];
  catalog: NodeCatalog;
  issues: FieldIssue[];
  allowExec?: boolean;
  showTypes?: boolean;
  onChange: (pins: FunctionPin[]) => void;
}

function defaultPinName(singular: string): string {
  switch (singular) {
    case "return value":
      return "Result";
    case "field":
      return "Field";
    case "value":
      return "Value";
    case "output":
      return "Out";
    case "input":
      return "In";
    default:
      return "Value";
  }
}

export function PinList({ title, singular, field, pins, catalog, issues, allowExec = false, showTypes = true, onChange }: PinListProps) {
  const update = (index: number, patch: Partial<FunctionPin>) =>
    onChange(pins.map((pin, position) => (position === index ? { ...pin, ...patch } : pin)));
  const swap = (index: number, other: number) => {
    const next = [...pins];
    [next[index], next[other]] = [next[other], next[index]];
    onChange(next);
  };
  return (
    <div className={styles.section}>
      <div className={styles.sectionHeader}>
        <span>{title}</span>
        <button
          type="button"
          className={styles.add}
          onClick={() =>
            onChange([...pins, { id: generateUlid().slice(-8), name: `${defaultPinName(singular)}${pins.length + 1}`, type: showTypes ? "number" : "integer" }])
          }
        >
          <Plus size={13} /> Add
        </button>
      </div>
      {pins.length === 0 && <p className={styles.empty}>No {singular}s.</p>}
      {pins.map((pin, index) => {
        const pinIssues = issues.filter((issue) => String(issue.field) === `${field}.${index}`);
        return (
          <div key={pin.id} className={styles.pinRow}>
            <div className={styles.pinControls}>
              <input
                className={styles.name}
                value={pin.name}
                aria-label={`${singular} name`}
                aria-invalid={pinIssues.length > 0}
                spellCheck={false}
                onChange={(event) => update(index, { name: event.target.value })}
              />
              {showTypes && (
                <TypePicker catalog={catalog} value={pin.type} label={pin.name} allowExec={allowExec} onChange={(type) => update(index, { type })} />
              )}
              <span className={styles.rowActions}>
                <button type="button" title="Move up" disabled={index === 0} onClick={() => swap(index, index - 1)}>
                  <ArrowUp size={13} />
                </button>
                <button type="button" title="Move down" disabled={index === pins.length - 1} onClick={() => swap(index, index + 1)}>
                  <ArrowDown size={13} />
                </button>
                <button
                  type="button"
                  title={`Remove ${singular}`}
                  onClick={() => onChange(pins.filter((_, position) => position !== index))}
                >
                  <Trash2 size={13} />
                </button>
              </span>
            </div>
            {pinIssues.map((issue, issueIndex) => (
              <IssueMessage key={issueIndex} issue={issue} />
            ))}
          </div>
        );
      })}
    </div>
  );
}

export function FunctionSignatureEditor({ variant, libraryName, name, catalog, signature, issues, onChange }: FunctionSignatureEditorProps) {
  const isMacro = variant === "macro";
  const luaName = (name.match(/[A-Za-z0-9]+/g) ?? []).map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join("");
  return (
    <div className={styles.editor}>
      <div className={styles.kind}>{isMacro ? "Macro" : variant === "method" ? "Method" : "Function"}</div>
      <h3 className={styles.title}>{name}</h3>
      <textarea
        className={styles.description}
        value={signature.description ?? ""}
        placeholder={`What does this ${isMacro ? "macro" : variant} do? Shown in the node menu and tooltips.`}
        rows={2}
        onChange={(event) => onChange({ ...signature, description: event.target.value })}
      />
      {!isMacro && (
        <label className={styles.pure}>
          <input
            type="checkbox"
            checked={signature.pure ?? false}
            onChange={(event) => onChange({ ...signature, pure: event.target.checked })}
          />
          <span>
            <strong>Pure</strong>
            <span>No execution pins. Use it for calculations without side effects, it runs whenever its result is needed.</span>
          </span>
        </label>
      )}
      {variant === "function" && (
        <label className={styles.pure}>
          <input
            type="checkbox"
            checked={signature.exported ?? false}
            onChange={(event) => onChange({ ...signature, exported: event.target.checked })}
          />
          <span>
            <strong>Export to other packages</strong>
            <span>
              Packages that list this package in their requirements can call it. In Lua it is <code>{libraryName}.{luaName}</code>.
            </span>
          </span>
        </label>
      )}
      <PinList
        title={isMacro ? "Inputs" : "Parameters"}
        singular={isMacro ? "input" : "parameter"}
        field="parameters"
        pins={signature.parameters ?? []}
        catalog={catalog}
        issues={issues}
        allowExec={isMacro}
        onChange={(parameters) => onChange({ ...signature, parameters })}
      />
      <PinList
        title={isMacro ? "Outputs" : "Return values"}
        singular={isMacro ? "output" : "return value"}
        field="returns"
        pins={signature.returns ?? []}
        catalog={catalog}
        issues={issues}
        allowExec={isMacro}
        onChange={(returns) => onChange({ ...signature, returns })}
      />
      <p className={styles.note}>
        {isMacro
          ? "A macro is copied into every graph that uses it, so it can have several execution inputs and outputs, for example a loop with Body and Completed. Use a function for large logic, it exists only once."
          : "Renaming a parameter keeps every existing connection, also in graphs that call this function."}
      </p>
    </div>
  );
}
