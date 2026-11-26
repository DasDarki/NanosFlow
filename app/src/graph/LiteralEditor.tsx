import { useEffect, useState } from "react";
import { baseType, STRUCT_FIELDS, type NodeCatalog } from "./catalog";
import type { LiteralValue, PinDefinition } from "./model";
import styles from "./FlowNode.module.css";

interface LiteralEditorProps {
  catalog: NodeCatalog;
  pin: PinDefinition;
  value: LiteralValue | undefined;
  onChange: (value: LiteralValue | undefined) => void;
}

function unquote(value: string): string {
  const trimmed = value.trim();
  if (trimmed.length >= 2 && (trimmed.startsWith('"') || trimmed.startsWith("'")) && trimmed.endsWith(trimmed[0])) {
    return trimmed.slice(1, -1);
  }
  return trimmed;
}

function NumberField({
  value,
  placeholder,
  integer,
  label,
  onChange,
}: {
  value: number | undefined;
  placeholder: string;
  integer: boolean;
  label: string;
  onChange: (value: number | undefined) => void;
}) {
  const [text, setText] = useState(value === undefined ? "" : String(value));

  useEffect(() => {
    setText((current) => (Number(current) === value && current !== "" ? current : value === undefined ? "" : String(value)));
  }, [value]);

  return (
    <input
      className={`${styles.literal} ${styles.literalNumber} nodrag`}
      inputMode="decimal"
      value={text}
      placeholder={placeholder}
      aria-label={label}
      spellCheck={false}
      onChange={(event) => {
        const next = event.target.value;
        setText(next);
        if (next.trim() === "") {
          onChange(undefined);
          return;
        }
        const parsed = Number(next.replace(",", "."));
        if (Number.isFinite(parsed)) {
          onChange(integer ? Math.trunc(parsed) : parsed);
        }
      }}
      data-invalid={text.trim() !== "" && !Number.isFinite(Number(text.replace(",", ".")))}
    />
  );
}

export function LiteralEditor({ catalog, pin, value, onChange }: LiteralEditorProps) {
  if (pin.type.kind === "exec") {
    return null;
  }
  const base = baseType(pin.type.type);
  if (base.kind !== "named") {
    return null;
  }
  const category = catalog.category(base.of);
  const fallback = pin.default ?? "";

  switch (category) {
    case "boolean": {
      const checked = typeof value === "boolean" ? value : fallback.trim() === "true";
      return (
        <input
          type="checkbox"
          className={`${styles.checkbox} nodrag`}
          checked={checked}
          aria-label={pin.label || pin.name}
          onChange={(event) => onChange(event.target.checked)}
        />
      );
    }
    case "number":
    case "integer":
      return (
        <NumberField
          value={typeof value === "number" ? value : undefined}
          placeholder={Number.isFinite(Number(fallback)) && fallback !== "" ? fallback : "0"}
          integer={category === "integer"}
          label={pin.label || pin.name}
          onChange={onChange}
        />
      );
    case "string":
    case "asset-path":
      return (
        <input
          className={`${styles.literal} nodrag`}
          value={typeof value === "string" ? value : ""}
          placeholder={fallback ? unquote(fallback) : category === "asset-path" ? "asset-pack::AssetKey" : "text"}
          aria-label={pin.label || pin.name}
          spellCheck={false}
          onChange={(event) => onChange(event.target.value === "" ? undefined : event.target.value)}
        />
      );
    case "enum": {
      const info = catalog.enumInfo(base.of);
      const defaultKey = fallback.includes(".") ? fallback.split(".").pop() : undefined;
      const selected = typeof value === "string" ? value : "";
      return (
        <select
          className={`${styles.literal} ${styles.select} nodrag`}
          value={selected}
          aria-label={pin.label || pin.name}
          onChange={(event) => onChange(event.target.value === "" ? undefined : event.target.value)}
        >
          <option value="">{defaultKey ? `${defaultKey} (default)` : "Choose..."}</option>
          {info?.values.map((entry) => (
            <option key={entry.key} value={entry.key} title={entry.description}>
              {entry.label ?? entry.key}
            </option>
          ))}
        </select>
      );
    }
    case "struct": {
      const fields = STRUCT_FIELDS[base.of];
      if (!fields) {
        return null;
      }
      const current = typeof value === "object" && value !== null ? value : {};
      return (
        <span className={styles.structFields}>
          {fields.map((field) => (
            <span key={field} className={styles.structField}>
              <span className={styles.structLabel}>{field}</span>
              <NumberField
                value={current[field]}
                placeholder={base.of === "Color" && field === "A" ? "1" : "0"}
                integer={false}
                label={`${pin.label || pin.name} ${field}`}
                onChange={(next) => {
                  const updated = { ...current };
                  if (next === undefined) {
                    delete updated[field];
                  } else {
                    updated[field] = next;
                  }
                  onChange(Object.keys(updated).length === 0 ? undefined : updated);
                }}
              />
            </span>
          ))}
        </span>
      );
    }
    default:
      return null;
  }
}
