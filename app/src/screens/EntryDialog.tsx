import { Blocks, Boxes, ListOrdered, Plug, Radio, Shapes, SquareFunction, Workflow } from "lucide-react";
import { useEffect, useState } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { ChoiceCard, ChoiceGrid } from "../components/Choice";
import { Field, TextInput } from "../components/Field";
import type { FieldIssue, GraphKind } from "../types";
import styles from "./EntryDialog.module.css";

export type EntryDialogMode =
  | { type: "create-graph"; kind: GraphKind }
  | { type: "create-method"; className: string }
  | { type: "create-dispatcher"; className: string }
  | { type: "create-folder" }
  | { type: "collapse"; className: string | null; nodeCount: number }
  | { type: "rename"; isFolder: boolean; isClass: boolean; currentName: string };

export interface ParentOptions {
  api: string[];
  project: string[];
}

interface EntryDialogProps {
  mode: EntryDialogMode;
  locationLabel: string;
  parentOptions: ParentOptions;
  onCancel: () => void;
  onSubmit: (name: string, kind: GraphKind, parent: string) => Promise<void>;
}

const KIND_HINTS: Record<GraphKind, string> = {
  "event-graph": "Reacts to events like Player Ready or Character Death. Becomes its own Lua file.",
  function: "Reusable logic with parameters and return values. Callable from every graph in this package.",
  class: "Your own entity type based on Character, Prop, Weapon and more, with member variables and methods.",
  event: "A message between client and server, or between graphs of one side, with its own parameters.",
  struct: "Your own data type with named fields, for example player stats or an inventory slot.",
  enum: "A fixed list of named values, for example round phases or teams. Pick them on pins and switch on them.",
  interface: "Functions that several classes promise to have. Call them on anything, without knowing its class.",
  dispatcher: "Something an instance of a class announces, for example On Died. Other graphs bind to it and react.",
  macro: "Nodes that are copied into every graph that uses them. Can have several execution pins, like a custom loop.",
};

const CLASS_NAME_PATTERN = /^[A-Za-z][A-Za-z0-9_]*$/;

export function EntryDialog({ mode, locationLabel, parentOptions, onCancel, onSubmit }: EntryDialogProps) {
  const [name, setName] = useState(mode.type === "rename" ? mode.currentName : "");
  const [kind, setKind] = useState<GraphKind>(mode.type === "create-graph" ? mode.kind : "event-graph");
  const [parent, setParent] = useState("Character");
  const [nameIssues, setNameIssues] = useState<FieldIssue[]>([]);
  const [serverIssue, setServerIssue] = useState<FieldIssue | null>(null);
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setServerIssue(null);
    void api.validateEntryName(name).then((result) => {
      if (!cancelled) {
        setNameIssues(result);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [name]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onCancel();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  const isClass =
    (mode.type === "create-graph" && (kind === "class" || kind === "struct" || kind === "enum" || kind === "interface")) || (mode.type === "rename" && mode.isClass);
  const issues: FieldIssue[] =
    nameIssues.length === 0 && isClass && name.length > 0 && !CLASS_NAME_PATTERN.test(name)
      ? [
          {
            field: "name",
            severity: "error",
            message: kind === "struct" || kind === "enum" || kind === "interface"
              ? "Type names: letters, digits and '_' only, starting with a letter. For example 'PlayerStats' or 'RoundPhase'."
              : "Class names become Lua identifiers: letters, digits and '_' only, starting with a letter. For example 'ZombieCharacter'.",
          },
        ]
      : nameIssues;

  const title =
    mode.type === "create-folder"
      ? "New folder"
      : mode.type === "collapse"
        ? mode.className
          ? `Collapse into a method of ${mode.className}`
          : "Collapse into a function"
      : mode.type === "create-method"
        ? `New method of ${mode.className}`
      : mode.type === "create-dispatcher"
        ? `New event dispatcher of ${mode.className}`
        : mode.type === "rename"
          ? `Rename ${mode.isFolder ? "folder" : mode.isClass ? "class" : "graph"}`
          : kind === "function"
            ? "New function"
            : kind === "macro"
              ? "New macro"
              : kind === "class"
              ? "New class"
              : kind === "event"
                ? "New network event"
                : kind === "struct"
                  ? "New struct"
                  : kind === "enum"
                  ? "New enum"
                  : kind === "interface"
                  ? "New interface"
                  : "New event graph";

  async function submit() {
    setTouched(true);
    if (issues.length > 0 || busy) {
      return;
    }
    if (mode.type === "rename" && name === mode.currentName) {
      onCancel();
      return;
    }
    setBusy(true);
    try {
      await onSubmit(name, kind, parent);
    } catch (error) {
      const commandError = toCommandError(error);
      setServerIssue(commandError.issues[0] ?? { field: "name", severity: "error", message: commandError.message });
      setBusy(false);
    }
  }

  const classIssue = isClass && name.length > 0 && !CLASS_NAME_PATTERN.test(name) ? issues[0] : undefined;
  const issue = serverIssue ?? (touched ? issues[0] : classIssue);

  return (
    <div className={styles.backdrop} onMouseDown={onCancel}>
      <form
        className={styles.dialog}
        role="dialog"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <div>
          <h2 className={styles.title}>{title}</h2>
          <p className={styles.location}>
            in <code>{locationLabel}</code>
          </p>
        </div>
        {mode.type === "collapse" && (
          <p className={styles.explanation}>
            {mode.nodeCount} {mode.nodeCount === 1 ? "node moves" : "nodes move"} into the new {mode.className ? "method" : "function"} and
            a call to it takes their place. Wires that cross the selection become its parameters and return values.
          </p>
        )}
        {mode.type === "create-graph" && (
          <ChoiceGrid columns={2}>
            <ChoiceCard
              title="Event graph"
              description={KIND_HINTS["event-graph"]}
              icon={<Workflow size={17} />}
              selected={kind === "event-graph"}
              onSelect={() => setKind("event-graph")}
            />
            <ChoiceCard
              title="Function"
              description={KIND_HINTS.function}
              icon={<SquareFunction size={17} />}
              tint="#9b4dca"
              selected={kind === "function"}
              onSelect={() => setKind("function")}
            />
            <ChoiceCard
              title="Macro"
              description={KIND_HINTS.macro}
              icon={<Blocks size={17} />}
              tint="#6fd3c1"
              selected={kind === "macro"}
              onSelect={() => setKind("macro")}
            />
            <ChoiceCard
              title="Class"
              description={KIND_HINTS.class}
              icon={<Boxes size={17} />}
              tint="#e0a458"
              selected={kind === "class"}
              onSelect={() => setKind("class")}
            />
            <ChoiceCard
              title="Network event"
              description={KIND_HINTS.event}
              icon={<Radio size={17} />}
              tint="#f472b6"
              selected={kind === "event"}
              onSelect={() => setKind("event")}
            />
            <ChoiceCard
              title="Struct"
              description={KIND_HINTS.struct}
              icon={<Shapes size={17} />}
              tint="#f6a96b"
              selected={kind === "struct"}
              onSelect={() => setKind("struct")}
            />
            <ChoiceCard
              title="Enum"
              description={KIND_HINTS.enum}
              icon={<ListOrdered size={17} />}
              tint="#a3e635"
              selected={kind === "enum"}
              onSelect={() => setKind("enum")}
            />
            <ChoiceCard
              title="Interface"
              description={KIND_HINTS.interface}
              icon={<Plug size={17} />}
              tint="#6fd3c1"
              selected={kind === "interface"}
              onSelect={() => setKind("interface")}
            />
          </ChoiceGrid>
        )}
        <Field label="Name" htmlFor="entry-name" issue={issue} hint={
            isClass
              ? mode.type === "rename"
                ? "Letters, digits and '_', starting with a letter. Every variable, pin and cast that uses the old name is updated."
                : kind === "struct" || kind === "enum" || kind === "interface"
                  ? `Letters, digits and '_', starting with a letter. This becomes the type name of the ${kind}.`
                  : "Letters, digits and '_', starting with a letter. This becomes the class name in Lua."
              : "Letters, digits, spaces, '-' and '_'. Also used as the Lua file name."
          }>
          <TextInput
            autoFocus
            id="entry-name"
            value={name}
            placeholder={
              mode.type === "create-folder"
                ? "Weapons"
                : mode.type === "create-dispatcher"
                  ? "On Died"
                : mode.type === "create-method" || mode.type === "collapse"
                  ? mode.type === "collapse" && !mode.className
                    ? "Calculate Damage"
                    : "Bite"
                  : isClass
                    ? kind === "struct"
                      ? "PlayerStats"
                      : kind === "enum"
                      ? "RoundPhase"
                      : kind === "interface"
                      ? "Damageable"
                      : "ZombieCharacter"
                    : kind === "function"
                      ? "Give Loadout"
                      : kind === "macro"
                        ? "For Each Alive Player"
                      : kind === "event"
                        ? "Player Shot"
                        : "Round Logic"
            }
            issue={issue}
            onBlur={() => setTouched(true)}
            onValueChange={setName}
          />
        </Field>
        {kind === "class" && mode.type === "create-graph" && (
          <Field label="Inherits from" htmlFor="entry-parent" hint="Your class gets every function, event and property of this class.">
            <select id="entry-parent" className={styles.select} value={parent} onChange={(event) => setParent(event.target.value)}>
              {parentOptions.project.length > 0 && (
                <optgroup label="Your classes">
                  {parentOptions.project.map((option) => (
                    <option key={option} value={option}>
                      {option}
                    </option>
                  ))}
                </optgroup>
              )}
              <optgroup label="nanos world">
                {parentOptions.api.map((option) => (
                  <option key={option} value={option}>
                    {option}
                  </option>
                ))}
              </optgroup>
            </select>
          </Field>
        )}
        <div className={styles.actions}>
          <Button type="button" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" busy={busy}>
            {mode.type === "rename" ? "Rename" : mode.type === "collapse" ? "Collapse" : "Create"}
          </Button>
        </div>
      </form>
    </div>
  );
}
