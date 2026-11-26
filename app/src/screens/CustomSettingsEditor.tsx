import { CircleHelp, Plus, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { TextInput } from "../components/Field";
import type { CustomSetting, CustomSettingKind, PackageSummary, ProjectView } from "../types";
import styles from "./CustomSettingsEditor.module.css";

interface Draft {
  id: number;
  key: string;
  label: string;
  type: CustomSettingKind;
  description: string;
  value: string;
  options: string;
}

interface CustomSettingsEditorProps {
  project: ProjectView;
  pkg: PackageSummary;
  onProjectChange: (project: ProjectView) => void;
  onOpenHelp?: () => void;
}

const TYPES: { value: CustomSettingKind; label: string }[] = [
  { value: "boolean", label: "Boolean" },
  { value: "integer", label: "Integer" },
  { value: "floating", label: "Floating" },
  { value: "select", label: "Select" },
  { value: "text", label: "Text" },
];

let nextDraftId = 1;

function toDraft(setting: CustomSetting): Draft {
  return {
    id: nextDraftId++,
    key: setting.key,
    label: setting.label,
    type: setting.type,
    description: setting.description ?? "",
    value: String(setting.default),
    options: (setting.options ?? []).join(", "),
  };
}

function parseOptions(text: string): string[] {
  return text
    .split(",")
    .map((option) => option.trim())
    .filter(Boolean);
}

function fromDraft(draft: Draft): CustomSetting {
  const value = draft.value.trim();
  const defaultValue: CustomSetting["default"] =
    draft.type === "boolean"
      ? value === "true"
      : draft.type === "integer"
        ? Math.trunc(Number(value) || 0)
        : draft.type === "floating"
          ? Number(value) || 0
          : draft.value;
  return {
    key: draft.key.trim(),
    label: draft.label.trim(),
    type: draft.type,
    description: draft.description.trim() || undefined,
    default: defaultValue,
    options: draft.type === "select" ? parseOptions(draft.options) : undefined,
  };
}

function suggestKey(existing: Draft[]): string {
  let index = existing.length + 1;
  while (existing.some((draft) => draft.key === `setting_${index}`)) {
    index += 1;
  }
  return `setting_${index}`;
}

export function CustomSettingsEditor({ project, pkg, onProjectChange, onOpenHelp }: CustomSettingsEditorProps) {
  const [drafts, setDrafts] = useState<Draft[]>(() => pkg.customSettings.map(toDraft));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setDrafts(pkg.customSettings.map(toDraft));
  }, [pkg.customSettings]);

  const saved = useMemo(() => JSON.stringify(pkg.customSettings.map(toDraft).map(fromDraft)), [pkg.customSettings]);
  const dirty = JSON.stringify(drafts.map(fromDraft)) !== saved;

  const change = (id: number, patch: Partial<Draft>) => setDrafts((current) => current.map((draft) => (draft.id === id ? { ...draft, ...patch } : draft)));

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      onProjectChange(await api.updateCustomSettings(project.projectFile, pkg.path, drafts.map(fromDraft)));
    } catch (reason) {
      const failure = toCommandError(reason);
      setError(failure.issues.length > 0 ? failure.issues.map((issue) => issue.message).join(" ") : failure.message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={styles.editor}>
      {drafts.length === 0 && <p className={styles.empty}>No custom settings yet.</p>}
      {drafts.map((draft) => (
        <div key={draft.id} className={styles.setting}>
          <div className={styles.line}>
            <TextInput mono value={draft.key} onValueChange={(key) => change(draft.id, { key })} aria-label="Key" placeholder="max_props" />
            <TextInput value={draft.label} onValueChange={(label) => change(draft.id, { label })} aria-label="Label" placeholder="Label, for example Max Props" />
            <select
              className={styles.select}
              value={draft.type}
              aria-label="Type"
              onChange={(event) => {
                const type = event.target.value as CustomSettingKind;
                const value = type === "boolean" ? "false" : type === "integer" || type === "floating" ? "0" : type === "select" ? (parseOptions(draft.options)[0] ?? "") : "";
                change(draft.id, { type, value });
              }}
            >
              {TYPES.map((type) => (
                <option key={type.value} value={type.value}>
                  {type.label}
                </option>
              ))}
            </select>
            <button type="button" className={styles.remove} onClick={() => setDrafts((current) => current.filter((entry) => entry.id !== draft.id))} aria-label={`Remove ${draft.key}`}>
              <Trash2 size={14} />
            </button>
          </div>
          <div className={styles.line}>
            {draft.type === "select" && (
              <TextInput
                value={draft.options}
                onValueChange={(options) => {
                  const list = parseOptions(options);
                  change(draft.id, { options, value: list.includes(draft.value) ? draft.value : (list[0] ?? "") });
                }}
                aria-label="Options"
                placeholder="Options, separated by commas"
              />
            )}
            <label className={styles.defaultField}>
              <span>Default</span>
              {draft.type === "boolean" ? (
                <select className={styles.select} value={draft.value} onChange={(event) => change(draft.id, { value: event.target.value })} aria-label="Default">
                  <option value="true">true</option>
                  <option value="false">false</option>
                </select>
              ) : draft.type === "select" ? (
                <select className={styles.select} value={draft.value} onChange={(event) => change(draft.id, { value: event.target.value })} aria-label="Default">
                  {parseOptions(draft.options).map((option) => (
                    <option key={option} value={option}>
                      {option}
                    </option>
                  ))}
                </select>
              ) : (
                <TextInput
                  mono={draft.type !== "text"}
                  inputMode={draft.type === "text" ? undefined : "decimal"}
                  value={draft.value}
                  onValueChange={(value) => change(draft.id, { value })}
                  aria-label="Default"
                />
              )}
            </label>
          </div>
          <TextInput value={draft.description} onValueChange={(description) => change(draft.id, { description })} aria-label="Description" placeholder="Description shown in the New Game screen" />
        </div>
      ))}
      {error && <p className={styles.error}>{error}</p>}
      <div className={styles.actions}>
        <Button
          variant="ghost"
          size="small"
          icon={<Plus size={14} />}
          onClick={() =>
            setDrafts((current) => [...current, { id: nextDraftId++, key: suggestKey(current), label: "", type: "integer", description: "", value: "0", options: "" }])
          }
        >
          Add setting
        </Button>
        {onOpenHelp && (
          <Button variant="ghost" size="small" icon={<CircleHelp size={14} />} onClick={onOpenHelp}>
            How it works
          </Button>
        )}
        <span className={styles.spacer} />
        {dirty && (
          <Button variant="ghost" size="small" onClick={() => setDrafts(pkg.customSettings.map(toDraft))}>
            Discard
          </Button>
        )}
        <Button variant="primary" size="small" disabled={!dirty} busy={busy} onClick={() => void save()}>
          Save settings
        </Button>
      </div>
    </div>
  );
}
