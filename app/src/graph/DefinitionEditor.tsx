import { ArrowLeftRight, Check, CircleAlert, CloudOff, ListOrdered, LoaderCircle, Megaphone, MonitorSmartphone, Radio, Server, Shapes } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { api, toCommandError } from "../api";
import { ChoiceCard, ChoiceGrid } from "../components/Choice";
import { useToasts } from "../components/Toasts";
import type { ApiChannel, CommandError, EventDirection, FieldIssue, GraphSummary, ScriptContext } from "../types";
import type { NodeCatalog } from "./catalog";
import { PinList } from "./FunctionSignatureEditor";
import type { FunctionSignature } from "./model";
import styles from "./DefinitionEditor.module.css";

interface DefinitionEditorProps {
  graph: GraphSummary;
  packagePath: string;
  apiChannel: ApiChannel;
  catalog: NodeCatalog;
  onRegisterFlush: (flush: (() => Promise<void>) | null) => void;
}

type SaveState = "saved" | "saving" | "failed";
type Side = "server" | "client" | "both";

const SAVE_DELAY = 500;

const DIRECTIONS: { value: EventDirection; title: string; description: string; icon: ReactNode; tint: string }[] = [
  {
    value: "to-server",
    title: "Client to server",
    description: "A client tells the server something, for example that the player pressed a key. The server also learns which player sent it.",
    icon: <Server size={17} />,
    tint: "#60a5fa",
  },
  {
    value: "to-clients",
    title: "Server to clients",
    description: "The server tells one player or everyone something, for example that a round started or the score changed.",
    icon: <MonitorSmartphone size={17} />,
    tint: "#c084fc",
  },
  {
    value: "local",
    title: "Local",
    description: "Stays on the side that calls it. Use it to let graphs of the same side react to each other without calling them directly.",
    icon: <ArrowLeftRight size={17} />,
    tint: "#22d3ee",
  },
];

function words(text: string): string[] {
  return text
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/([A-Z])([A-Z][a-z])/g, "$1 $2")
    .split(/[^A-Za-z0-9]+/)
    .filter(Boolean);
}

function pascalCase(text: string): string {
  return words(text)
    .map((word) => word[0].toUpperCase() + word.slice(1))
    .join("");
}

function snakeCase(text: string): string {
  const joined = words(text)
    .map((word) => word.toLowerCase())
    .join("_");
  return joined || "value";
}

function localSide(context: ScriptContext): Side {
  return context === "shared" ? "both" : context;
}

function sideLabel(side: Side): string {
  return side === "both" ? "server and client" : side;
}

interface NodeSummary {
  title: string;
  side: Side;
  note: string;
}

function nodesFor(name: string, direction: EventDirection, context: ScriptContext): NodeSummary[] {
  switch (direction) {
    case "to-server":
      return [
        { title: `Send ${name} to Server`, side: "client", note: "Sends the event from a client graph." },
        { title: `On ${name}`, side: "server", note: "Runs in server graphs when a client sends the event, with the sending player." },
      ];
    case "to-clients":
      return [
        { title: `Send ${name} to Player`, side: "server", note: "Sends the event to one player's client." },
        { title: `Send ${name} to All Players`, side: "server", note: "Sends the event to every connected client." },
        { title: `On ${name}`, side: "client", note: "Runs in client graphs when the server sends the event." },
      ];
    case "local":
      return [
        { title: `Call ${name}`, side: localSide(context), note: "Triggers the event on the same side." },
        { title: `On ${name}`, side: localSide(context), note: "Runs in every graph of that side that handles it." },
      ];
  }
}

function structNodes(name: string): NodeSummary[] {
  return [
    { title: `Make ${name}`, side: "both", note: "Creates a new value from its fields." },
    { title: `Break ${name}`, side: "both", note: "Splits a value into its fields." },
    { title: `Set Fields of ${name}`, side: "both", note: "Changes the connected fields of an existing value." },
  ];
}

function dispatcherNodes(name: string): NodeSummary[] {
  return [
    { title: `Bind to ${name}`, side: "both", note: "Its output runs every time the dispatcher is called on that instance." },
    { title: `Call ${name}`, side: "both", note: "Runs everything that is bound, with the parameter values." },
    { title: `Unbind from ${name}`, side: "both", note: "Removes one binding, using the Binding output of Bind." },
    { title: `Unbind All from ${name}`, side: "both", note: "Removes every binding on that instance." },
  ];
}

function dispatcherExample(name: string, signature: FunctionSignature): { label: string; code: string }[] {
  const member = pascalCase(name) || "Dispatcher";
  const parameters = (signature.parameters ?? []).map((parameter) => snakeCase(parameter.name));
  return [
    { label: "Bind", code: `local binding = zombie:Bind${member}(function(${parameters.join(", ")})\n\t...\nend)` },
    { label: "Call and unbind", code: `zombie:Call${member}(${parameters.join(", ")})\nzombie:Unbind${member}(binding)` },
  ];
}

function enumNodes(name: string): NodeSummary[] {
  return [
    { title: `Switch on ${name}`, side: "both", note: "Continues with the output of the current value." },
    { title: `${name} pins`, side: "both", note: "Pick a value right on the pin, or compare values with Equal." },
  ];
}

function enumExample(name: string, signature: FunctionSignature): { label: string; code: string }[] {
  const values = (signature.parameters ?? []).map((value) => pascalCase(value.name) || "Value");
  const table = values.length > 0 ? `{\n${values.map((value, index) => `\t${value} = ${index + 1},`).join("\n")}\n}` : "{}";
  return [
    { label: "Definition", code: `${name} = ${table}` },
    { label: "Use", code: `if phase == ${name}.${values[0] ?? "Value"} then\n\t...\nend` },
  ];
}

function structExample(name: string, signature: FunctionSignature): { label: string; code: string }[] {
  const fields = signature.parameters ?? [];
  const local = snakeCase(name);
  const entries = fields.map((field) => `${pascalCase(field.name) || "Field"} = ...`).join(", ");
  const first = fields[0] ? pascalCase(fields[0].name) || "Field" : "Field";
  return [
    { label: "Make", code: `local ${local} = { ${entries} }` },
    { label: "Break and set", code: `local value = ${local}.${first}\n${local}.${first} = value` },
  ];
}

function luaExample(name: string, direction: EventDirection, signature: FunctionSignature): { label: string; code: string }[] {
  const eventName = JSON.stringify(pascalCase(name) || "Unnamed");
  const parameters = (signature.parameters ?? []).map((parameter) => snakeCase(parameter.name));
  const list = parameters.join(", ");
  const withPrefix = (prefix: string[]) => [...prefix, ...parameters].join(", ");
  switch (direction) {
    case "to-server":
      return [
        { label: "Client", code: `Events.CallRemote(${[eventName, ...parameters].join(", ")})` },
        { label: "Server", code: `Events.SubscribeRemote(${eventName}, function(${withPrefix(["player"])})\n\t...\nend)` },
      ];
    case "to-clients":
      return [
        {
          label: "Server",
          code: `Events.CallRemote(${[eventName, "player", ...parameters].join(", ")})\nEvents.BroadcastRemote(${[eventName, ...parameters].join(", ")})`,
        },
        { label: "Client", code: `Events.SubscribeRemote(${eventName}, function(${list})\n\t...\nend)` },
      ];
    case "local":
      return [
        { label: "Call", code: `Events.Call(${[eventName, ...parameters].join(", ")})` },
        { label: "Handle", code: `Events.Subscribe(${eventName}, function(${list})\n\t...\nend)` },
      ];
  }
}

export function DefinitionEditor({ graph, packagePath, apiChannel, catalog, onRegisterFlush }: DefinitionEditorProps) {
  const { reportError } = useToasts();
  const isStruct = graph.kind === "struct";
  const isEnum = graph.kind === "enum";
  const isDispatcher = graph.kind === "dispatcher";
  const isType = isStruct || isEnum || isDispatcher;
  const [scopedCatalog, setScopedCatalog] = useState(catalog);
  const [direction, setDirection] = useState<EventDirection>("to-server");
  const [signature, setSignature] = useState<FunctionSignature | null>(null);
  const [issues, setIssues] = useState<FieldIssue[]>([]);
  const [failure, setFailure] = useState<CommandError | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("saved");
  const dirtyRef = useRef(false);
  const latestRef = useRef<{ direction: EventDirection; signature: FunctionSignature | null }>({ direction, signature });
  latestRef.current = { direction, signature };

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.loadGraph(graph.path), api.graphScope(apiChannel, packagePath, graph.path)])
      .then(([document, scope]) => {
        if (cancelled) {
          return;
        }
        setScopedCatalog(catalog.extend(scope.definitions, scope.classes, scope.structs, scope.enums));
        const { manifest } = document;
        setDirection(manifest.direction ?? "to-server");
        setSignature({ description: manifest.description ?? "", parameters: manifest.parameters ?? [] });
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
    const { direction: currentDirection, signature: currentSignature } = latestRef.current;
    if (!dirtyRef.current || !currentSignature) {
      return;
    }
    dirtyRef.current = false;
    setSaveState("saving");
    try {
      if (isType) {
        await api.updateFunctionSignature(apiChannel, packagePath, graph.path, {
          description: currentSignature.description,
          parameters: currentSignature.parameters,
        });
      } else {
        await api.updateEvent(apiChannel, packagePath, graph.path, currentDirection, currentSignature);
      }
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
  }, [apiChannel, packagePath, graph.path, reportError, isType]);

  useEffect(() => {
    if (!dirtyRef.current) {
      return;
    }
    const timer = window.setTimeout(() => void save(), SAVE_DELAY);
    return () => window.clearTimeout(timer);
  }, [direction, signature, save]);

  useEffect(() => {
    onRegisterFlush(save);
    return () => {
      onRegisterFlush(null);
      void save();
    };
  }, [onRegisterFlush, save]);

  const change = (next: { direction?: EventDirection; signature?: FunctionSignature }) => {
    dirtyRef.current = true;
    if (next.direction) {
      setDirection(next.direction);
    }
    if (next.signature) {
      setSignature(next.signature);
    }
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
  if (!signature) {
    return (
      <div className={styles.failure}>
        <LoaderCircle size={22} className={styles.spin} />
      </div>
    );
  }

  return (
    <div className={styles.scroller}>
      <div className={styles.page} data-variant={isEnum ? "enum" : isStruct ? "struct" : "event"}>
        <header className={styles.header}>
          <span className={styles.icon}>
            {isDispatcher ? <Megaphone size={20} /> : isEnum ? <ListOrdered size={20} /> : isStruct ? <Shapes size={20} /> : <Radio size={20} />}
          </span>
          <div className={styles.titles}>
            <span className={styles.kind}>{isDispatcher ? "Event dispatcher" : isEnum ? "Enum" : isStruct ? "Struct" : "Network event"}</span>
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
          value={signature.description ?? ""}
          placeholder={
            isDispatcher
              ? "When is this dispatcher called? Shown on its nodes and in the node menu."
              : isEnum
              ? "What does this enum list? Shown on its nodes and in the node menu."
              : isStruct
              ? "What does this struct hold? Shown on its nodes and in the node menu."
              : "What does this event mean? Shown on its nodes and in the node menu."
          }
          rows={2}
          onChange={(event) => change({ signature: { ...signature, description: event.target.value } })}
        />

        {!isType && (
          <section className={styles.section}>
            <h2>Direction</h2>
            <ChoiceGrid columns={3}>
              {DIRECTIONS.map((option) => (
                <ChoiceCard
                  key={option.value}
                  title={option.title}
                  description={option.description}
                  icon={option.icon}
                  tint={option.tint}
                  selected={direction === option.value}
                  onSelect={() => change({ direction: option.value })}
                />
              ))}
            </ChoiceGrid>
          </section>
        )}

        <section className={styles.section}>
          <PinList
            title={isEnum ? "Values" : isStruct ? "Fields" : "Parameters"}
            singular={isEnum ? "value" : isStruct ? "field" : "parameter"}
            showTypes={!isEnum}
            field="parameters"
            pins={signature.parameters ?? []}
            catalog={scopedCatalog}
            issues={issues}
            onChange={(parameters) => change({ signature: { ...signature, parameters } })}
          />
          <p className={styles.note}>
            {isDispatcher
              ? "Every binding receives these values. Bindings belong to one instance, so binding to one zombie does not react to another."
              : isEnum
              ? "In Lua the enum is a table of numbers. Pins store the value itself, so renaming or reordering values keeps every graph working."
              : isStruct
              ? "In Lua a struct is a plain table, so it can be stored in variables, arrays, maps and sent in network events."
              : "Parameters travel over the network, so prefer simple values like numbers, text, vectors and entities."}
          </p>
        </section>

        <section className={styles.section}>
          <h2>Nodes</h2>
          <ul className={styles.nodes}>
            {(isDispatcher ? dispatcherNodes(graph.name) : isEnum ? enumNodes(graph.name) : isStruct ? structNodes(graph.name) : nodesFor(graph.name, direction, graph.context)).map((node) => (
              <li key={node.title}>
                <span className={styles.nodeTitle}>{node.title}</span>
                <span className={styles.side} data-side={node.side}>
                  {sideLabel(node.side)}
                </span>
                <span className={styles.nodeNote}>{node.note}</span>
              </li>
            ))}
          </ul>
          <p className={styles.note}>Find them in the node menu of any graph by searching for {graph.name}.</p>
        </section>

        <section className={styles.section}>
          <h2>Lua</h2>
          <div className={styles.examples}>
            {(isDispatcher ? dispatcherExample(graph.name, signature) : isEnum ? enumExample(graph.name, signature) : isStruct ? structExample(graph.name, signature) : luaExample(graph.name, direction, signature)).map((example) => (
              <div key={example.label} className={styles.example}>
                <span>{example.label}</span>
                <pre>{example.code}</pre>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
