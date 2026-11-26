import { useMemo, type CSSProperties } from "react";
import type { NodeCatalog } from "./catalog";
import { pinColor } from "./pinStyle";
import styles from "./FunctionSignatureEditor.module.css";

const BASIC_TYPES = ["boolean", "number", "integer", "string", "table", "function", "any"];

interface TypePickerProps {
  catalog: NodeCatalog;
  value: string;
  onChange: (value: string) => void;
  label: string;
  allowExec?: boolean;
}

export const EXEC_TYPE = "exec";

export function TypePicker({ catalog, value, onChange, label, allowExec = false }: TypePickerProps) {
  const isArray = value.endsWith("[]");
  const base = isArray ? value.slice(0, -2) : value;
  const isExec = base === EXEC_TYPE;
  const groups = useMemo(
    () => [
      ...(allowExec ? [{ label: "Flow", names: [EXEC_TYPE] }] : []),
      { label: "Basic", names: BASIC_TYPES },
      { label: "Your structs", names: catalog.projectStructNames().sort() },
      { label: "Your enums", names: catalog.projectEnumNames().sort() },
      { label: "Your classes", names: catalog.projectClassNames().sort() },
      { label: "Structs", names: catalog.classNames("struct").sort() },
      { label: "Entities", names: catalog.apiClassNames().sort() },
      { label: "Enums", names: catalog.enumNames().sort() },
    ],
    [catalog, allowExec],
  );
  const known = groups.some((group) => group.names.includes(base));
  return (
    <span className={styles.typePicker}>
      <span
        className={styles.typeDot}
        style={{ "--pin": isExec ? pinColor(catalog, { kind: "exec" }) : pinColor(catalog, { kind: "data", type: { kind: "named", of: base } }) } as CSSProperties}
        data-exec={isExec}
        data-array={isArray}
      />
      <select
        className={styles.select}
        value={base}
        aria-label={`${label} type`}
        onChange={(event) => onChange(event.target.value + (isArray && event.target.value !== EXEC_TYPE ? "[]" : ""))}
      >
        {!known && <option value={base}>{base}</option>}
        {groups.filter((group) => group.names.length > 0).map((group) => (
          <optgroup key={group.label} label={group.label}>
            {group.names.map((name) => (
              <option key={name} value={name}>
                {name === EXEC_TYPE ? "Execution" : name}
              </option>
            ))}
          </optgroup>
        ))}
      </select>
      <button
        type="button"
        disabled={isExec}
        className={styles.arrayToggle}
        aria-pressed={isArray}
        title={isArray ? "Array of values. Click for a single value." : "Single value. Click to make it an array."}
        onClick={() => onChange(base + (isArray ? "" : "[]"))}
      >
        []
      </button>
    </span>
  );
}
