import { Check, CircleAlert, CloudOff, LoaderCircle, Plug, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, toCommandError } from "../api";
import { IssueMessage } from "../components/Field";
import { useToasts } from "../components/Toasts";
import type { ApiChannel, CommandError, FieldIssue, GraphSummary } from "../types";
import type { NodeCatalog } from "./catalog";
import { PinList } from "./FunctionSignatureEditor";
import type { InterfaceFunction } from "./model";
import { generateUlid } from "./ulid";
import styles from "./DefinitionEditor.module.css";
import own from "./InterfaceEditor.module.css";

interface InterfaceEditorProps {
  graph: GraphSummary;
  packagePath: string;
  apiChannel: ApiChannel;
  catalog: NodeCatalog;
  onRegisterFlush: (flush: (() => Promise<void>) | null) => void;
}

type SaveState = "saved" | "saving" | "failed";

interface Draft {
  description: string;
  functions: InterfaceFunction[];
}

const SAVE_DELAY = 500;

function issuesOf(issues: FieldIssue[], index: number): FieldIssue[] {
  const prefix = `functions.${index}.`;
  return issues
    .filter((issue) => String(issue.field).startsWith(prefix))
    .map((issue) => ({ ...issue, field: String(issue.field).slice(prefix.length) as FieldIssue["field"] }));
}

export function InterfaceEditor({ graph, packagePath, apiChannel, catalog, onRegisterFlush }: InterfaceEditorProps) {
  const { reportError } = useToasts();
  const [scopedCatalog, setScopedCatalog] = useState(catalog);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [issues, setIssues] = useState<FieldIssue[]>([]);
  const [failure, setFailure] = useState<CommandError | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("saved");
  const dirtyRef = useRef(false);
  const latestRef = useRef<Draft | null>(draft);
  latestRef.current = draft;

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.loadGraph(graph.path), api.graphScope(apiChannel, packagePath, graph.path)])
      .then(([document, scope]) => {
        if (cancelled) {
          return;
        }
        setScopedCatalog(catalog.extend(scope.definitions, scope.classes, scope.structs, scope.enums));
        setDraft({ description: document.manifest.description ?? "", functions: document.manifest.functions ?? [] });
      })
      .catch((error) => {
        if (!cancelled) {
          setFailure(toCommandError(error));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [graph.path, apiChannel, packagePath, catalog]);

  const save = useCallback(async () => {
    const current = latestRef.current;
    if (!dirtyRef.current || !current) {
      return;
    }
    dirtyRef.current = false;
    setSaveState("saving");
    try {
      await api.updateInterface(apiChannel, packagePath, graph.path, current.description, current.functions);
      setIssues([]);
      setSaveState("saved");
    } catch (error) {
      const commandError = toCommandError(error);
      setSaveState("failed");
      if (commandError.issues.length > 0) {
        setIssues(commandError.issues);
      } else {
        dirtyRef.current = true;
        reportError(commandError);
      }
    }
  }, [apiChannel, packagePath, graph.path, reportError]);

  useEffect(() => {
    if (!dirtyRef.current) {
      return;
    }
    const timer = window.setTimeout(() => void save(), SAVE_DELAY);
    return () => window.clearTimeout(timer);
  }, [draft, save]);

  useEffect(() => {
    onRegisterFlush(save);
    return () => {
      onRegisterFlush(null);
      void save();
    };
  }, [onRegisterFlush, save]);

  const change = (next: Draft) => {
    dirtyRef.current = true;
    setDraft(next);
  };

  if (failure) {
    return (
      <div className={styles.failure}>
        <CircleAlert size={28} />
        <h2>{failure.title}</h2>
        <p>{failure.message}</p>
      </div>
    );
  }
  if (!draft) {
    return (
      <div className={styles.failure}>
        <LoaderCircle size={22} className={styles.spin} />
      </div>
    );
  }

  const updateFunction = (index: number, patch: Partial<InterfaceFunction>) =>
    change({ ...draft, functions: draft.functions.map((member, position) => (position === index ? { ...member, ...patch } : member)) });

  return (
    <div className={styles.scroller}>
      <div className={`${styles.page} ${own.page}`}>
        <header className={styles.header}>
          <span className={styles.icon}>
            <Plug size={20} />
          </span>
          <div className={styles.titles}>
            <span className={styles.kind}>Interface</span>
            <h1>{graph.name}</h1>
          </div>
          <span className={styles.saveState} data-state={saveState} aria-live="polite">
            {saveState === "saving" && <LoaderCircle size={13} className={styles.spin} />}
            {saveState === "saved" && <Check size={13} />}
            {saveState === "failed" && <CloudOff size={13} />}
            {saveState === "saving" ? "Saving" : saveState === "saved" ? "Saved" : "Not saved"}
          </span>
        </header>

        <textarea
          className={styles.description}
          value={draft.description}
          placeholder="What can classes with this interface do? Shown on its nodes and in the node menu."
          rows={2}
          onChange={(event) => change({ ...draft, description: event.target.value })}
        />

        <section className={styles.section}>
          <div className={own.sectionHeader}>
            <h2>Functions</h2>
            <button
              type="button"
              className={own.add}
              onClick={() =>
                change({
                  ...draft,
                  functions: [...draft.functions, { id: generateUlid().slice(-8), name: `Function${draft.functions.length + 1}`, parameters: [], returns: [] }],
                })
              }
            >
              <Plus size={13} /> Add function
            </button>
          </div>
          {draft.functions.length === 0 && <p className={styles.note}>No functions yet. Add the things every implementing class must be able to do.</p>}
          {draft.functions.map((member, index) => {
            const functionIssues = issuesOf(issues, index);
            const nameIssue = issues.find((issue) => String(issue.field) === `functions.${index}`);
            return (
              <div key={member.id} className={own.function}>
                <div className={own.functionHeader}>
                  <input
                    className={own.name}
                    value={member.name}
                    aria-label="Function name"
                    aria-invalid={Boolean(nameIssue)}
                    spellCheck={false}
                    onChange={(event) => updateFunction(index, { name: event.target.value })}
                  />
                  <button
                    type="button"
                    className={own.remove}
                    title="Remove this function"
                    onClick={() => change({ ...draft, functions: draft.functions.filter((_, position) => position !== index) })}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
                {nameIssue && <IssueMessage issue={nameIssue} />}
                <input
                  className={own.functionDescription}
                  value={member.description ?? ""}
                  placeholder="What does it do?"
                  onChange={(event) => updateFunction(index, { description: event.target.value })}
                />
                <PinList
                  title="Parameters"
                  singular="parameter"
                  field="parameters"
                  pins={member.parameters ?? []}
                  catalog={scopedCatalog}
                  issues={functionIssues}
                  onChange={(parameters) => updateFunction(index, { parameters })}
                />
                <PinList
                  title="Return values"
                  singular="return value"
                  field="returns"
                  pins={member.returns ?? []}
                  catalog={scopedCatalog}
                  issues={functionIssues}
                  onChange={(returns) => updateFunction(index, { returns })}
                />
              </div>
            );
          })}
          <p className={styles.note}>
            A class implements the interface in its class settings. NanosFlow then adds a method with the same name and pins
            for every function that is missing. The nodes of these functions work on any value: if it does not implement the
            interface, nothing happens.
          </p>
        </section>
      </div>
    </div>
  );
}
