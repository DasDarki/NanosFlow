import { Handle, Position, type NodeProps } from "@xyflow/react";
import {
  Braces,
  Calculator,
  CircleAlert,
  Dot,
  Eye,
  GitBranch,
  Hammer,
  LogIn,
  LogOut,
  MessageSquareText,
  ChartSpline,
  Minus,
  Plus,
  X,
  Quote,
  SquareFunction,
  TriangleAlert,
  Variable,
  Zap,
} from "lucide-react";
import { memo, useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { formatType, sideRunsIn } from "./catalog";
import { CASE_PREFIX, CASES_PROPERTY, COUNT_PROPERTY, pinCount, switchCases, timelineLength, timelineTracks, type SwitchCase } from "./dynamic";
import { generateUlid } from "./ulid";
import type { FlowNode } from "./convert";
import { pinKey, useEditor } from "./EditorContext";
import { LiteralEditor } from "./LiteralEditor";
import type { Diagnostic, NodeDefinition, NodeKind, PinDefinition } from "./model";
import { definitionColor, isArrayPin, pinColor } from "./pinStyle";
import styles from "./FlowNode.module.css";

const KIND_ICONS: Record<NodeKind, ReactNode> = {
  event: <Zap size={13} />,
  function: <SquareFunction size={13} />,
  "pure-function": <Calculator size={13} />,
  constructor: <Hammer size={13} />,
  "flow-control": <GitBranch size={13} />,
  operator: <Braces size={13} />,
  literal: <Quote size={13} />,
  "function-entry": <LogIn size={13} />,
  "function-return": <LogOut size={13} />,
  variable: <Variable size={13} />,
  comment: <MessageSquareText size={13} />,
  reroute: <Dot size={13} />,
};

function pinTitle(pin: PinDefinition, diagnostics: Diagnostic[]): string {
  const lines = [pin.type.kind === "exec" ? "Execution" : formatType(pin.type.type)];
  if (pin.description) {
    lines.push(pin.description);
  }
  if (pin.default !== undefined) {
    lines.push(`Default: ${pin.default === "" ? '""' : pin.default}`);
  }
  for (const diagnostic of diagnostics) {
    lines.push(`${diagnostic.severity === "error" ? "Error" : "Warning"}: ${diagnostic.message}`);
  }
  return lines.join("\n");
}

interface PinRowProps {
  nodeId: string;
  pin: PinDefinition;
  direction: "input" | "output";
  diagnostics: Diagnostic[];
  literal: ReactNode;
}

function PinRow({ nodeId, pin: declared, direction, diagnostics, literal }: PinRowProps) {
  const editor = useEditor();
  const pin = { ...declared, type: editor.pinType(nodeId, declared) };
  const connected = editor.connectedPins.has(pinKey(nodeId, direction, pin.name));
  const color = pinColor(editor.catalog, pin.type);
  const isExec = pin.type.kind === "exec";
  const severity = diagnostics.some((diagnostic) => diagnostic.severity === "error")
    ? "error"
    : diagnostics.length > 0
      ? "warning"
      : undefined;

  return (
    <div className={styles.pinRow} data-direction={direction} data-severity={severity} title={pinTitle(pin, diagnostics)}>
      <Handle
        type={direction === "input" ? "target" : "source"}
        position={direction === "input" ? Position.Left : Position.Right}
        id={pin.name}
        className={styles.handle}
        data-exec={isExec}
        data-array={isArrayPin(pin.type)}
        data-connected={connected}
        style={{ "--pin": color } as CSSProperties}
        onClick={(event) => {
          if (event.altKey) {
            event.stopPropagation();
            editor.disconnectPin(nodeId, pin.name, direction);
          }
        }}
      />
      {(pin.label || !isExec) && <span className={styles.pinLabel}>{pin.label}</span>}
      {literal}
    </div>
  );
}

function UnknownNode({ nodeType, selected, diagnostics }: { nodeType: string; selected: boolean; diagnostics: Diagnostic[] }) {
  return (
    <div className={styles.node} data-selected={selected} data-severity="error">
      <div className={styles.header} style={{ "--kind": "#7f1d1d" } as CSSProperties}>
        <span className={styles.icon}>
          <CircleAlert size={13} />
        </span>
        <span className={styles.titles}>
          <span className={styles.title}>Unknown node</span>
          <span className={styles.subtitle}>{nodeType}</span>
        </span>
      </div>
      <div className={styles.unknownBody}>{diagnostics[0]?.hint ?? "This node type does not exist in the selected API."}</div>
    </div>
  );
}

function CaseEditor({
  entry,
  numeric,
  onChange,
  onRemove,
}: {
  entry: SwitchCase;
  numeric: boolean;
  onChange: (value: number | string) => void;
  onRemove: () => void;
}) {
  const [text, setText] = useState(String(entry.value));
  useEffect(() => setText(String(entry.value)), [entry.value]);
  const commit = () => {
    if (numeric) {
      const parsed = Number.parseInt(text, 10);
      if (Number.isFinite(parsed) && parsed !== entry.value) {
        onChange(parsed);
      } else {
        setText(String(entry.value));
      }
    } else if (text !== entry.value) {
      onChange(text);
    }
  };
  return (
    <span className={`${styles.caseEditor} nodrag`}>
      <button type="button" className={styles.caseRemove} title="Remove this case" onClick={onRemove}>
        <X size={11} />
      </button>
      <input
        className={`${styles.literal} ${numeric ? styles.literalNumber : ""}`}
        value={text}
        aria-label="Case value"
        inputMode={numeric ? "numeric" : "text"}
        spellCheck={false}
        onChange={(event) => setText(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          event.stopPropagation();
          if (event.key === "Enter") {
            event.currentTarget.blur();
          }
        }}
      />
    </span>
  );
}

function DynamicBar({ id, definition, properties }: { id: string; definition: NodeDefinition; properties: Record<string, unknown> }) {
  const editor = useEditor();
  const dynamic = definition.dynamic;
  if (!dynamic) {
    return null;
  }
  if (dynamic.kind === "tracks") {
    const tracks = timelineTracks(properties);
    return (
      <div className={`${styles.dynamicBar} nodrag`}>
        <button type="button" onClick={() => editor.editTimeline(id)}>
          <ChartSpline size={12} /> Edit timeline
        </button>
        <span className={styles.dynamicInfo}>
          {timelineLength(properties)} s, {tracks.length} {tracks.length === 1 ? "track" : "tracks"}
          {properties.looping === true ? ", loops" : ""}
        </span>
      </div>
    );
  }
  if (dynamic.kind === "cases") {
    const cases = switchCases(properties);
    const numeric = definition.inputs.some((pin) => pin.name === "value" && pin.type.kind === "data" && pin.type.type.kind === "named" && pin.type.type.of === "integer");
    const next: number | string = numeric
      ? cases.reduce((highest, entry) => Math.max(highest, typeof entry.value === "number" ? entry.value + 1 : highest), 0)
      : `Option ${cases.length + 1}`;
    return (
      <div className={`${styles.dynamicBar} nodrag`}>
        <button
          type="button"
          onClick={() =>
            editor.setProperties(id, { [CASES_PROPERTY]: [...cases, { id: generateUlid().slice(-6).toLowerCase(), value: next }] })
          }
        >
          <Plus size={12} /> Add case
        </button>
      </div>
    );
  }
  const count = pinCount(definition, properties);
  return (
    <div className={`${styles.dynamicBar} nodrag`}>
      <button type="button" onClick={() => editor.setProperties(id, { [COUNT_PROPERTY]: count + 1 })}>
        <Plus size={12} /> Add {dynamic.kind === "exec-outputs" ? "output" : "element"}
      </button>
      <button
        type="button"
        title="Remove the last pin"
        disabled={count <= dynamic.minimum}
        onClick={() => editor.setProperties(id, { [COUNT_PROPERTY]: count - 1 })}
      >
        <Minus size={12} />
      </button>
    </div>
  );
}

export const FlowNodeView = memo(function FlowNodeView({ id, data, selected }: NodeProps<FlowNode>) {
  const editor = useEditor();
  const definition = editor.catalog.definitionFor(data.nodeType, data.properties);
  const diagnostics = editor.diagnosticsByNode.get(id) ?? [];

  if (!definition) {
    return <UnknownNode nodeType={data.nodeType} selected={selected} diagnostics={diagnostics} />;
  }

  const errors = diagnostics.filter((diagnostic) => diagnostic.severity === "error");
  const warnings = diagnostics.filter((diagnostic) => diagnostic.severity === "warning");
  const nodeLevel = diagnostics.filter((diagnostic) => !diagnostic.pin);
  const pinDiagnostics = (pin: string) => diagnostics.filter((diagnostic) => diagnostic.pin === pin);
  const isCompact = definition.kind === "operator" && definition.inputs.every((pin) => !pin.label);
  const wrongSide = !sideRunsIn(definition.side, editor.context);
  const runtime = editor.runtime[id];
  const isWatched = data.properties.watch === true;
  const hasBreakpoint = data.properties.breakpoint === true;

  return (
    <div
      className={styles.node}
      data-kind={definition.kind}
      data-selected={selected}
      data-severity={errors.length > 0 ? "error" : warnings.length > 0 ? "warning" : undefined}
      data-compact={isCompact}
      data-runtime-error={Boolean(runtime?.errors)}
      data-paused={Boolean(runtime?.paused)}
      data-change={editor.changes[id]}
      data-remote-selected={Boolean(editor.remoteSelection[id])}
      style={editor.remoteSelection[id] ? ({ "--remote": editor.remoteSelection[id].color } as CSSProperties) : undefined}
      title={editor.remoteSelection[id] ? `Selected by ${editor.remoteSelection[id].name}` : undefined}
    >
      {editor.changes[id] && (
        <span className={styles.changeBadge} data-change={editor.changes[id]}>
          {editor.changes[id] === "added" ? "New" : editor.changes[id] === "changed" ? "Changed" : "Moved"}
        </span>
      )}
      {hasBreakpoint && <span className={styles.breakpoint} title="Breakpoint. The server pauses here while NanosFlow is open (F9 to remove)." />}
      <div className={styles.header} style={{ "--kind": definitionColor(definition) } as CSSProperties}>
        <span className={styles.icon}>{KIND_ICONS[definition.kind]}</span>
        <span className={styles.titles}>
          <span className={styles.title}>{definition.title}</span>
          {definition.subtitle && <span className={styles.subtitle}>{definition.subtitle}</span>}
        </span>
        {definition.side !== "both" && (
          <span className={styles.side} data-side={definition.side} data-wrong={wrongSide} title={`Only available on the ${definition.side}`}>
            {definition.side === "server" ? "Server" : "Client"}
          </span>
        )}
        {(errors.length > 0 || warnings.length > 0) && (
          <span
            className={styles.problemBadge}
            data-severity={errors.length > 0 ? "error" : "warning"}
            title={diagnostics.map((diagnostic) => diagnostic.message).join("\n")}
          >
            {errors.length > 0 ? <CircleAlert size={12} /> : <TriangleAlert size={12} />}
            {errors.length + warnings.length}
          </span>
        )}
        {runtime && runtime.errors > 0 && (
          <span className={styles.runtimeBadge} title={`Failed at runtime:\n${runtime.lastError ?? ""}`}>
            <Zap size={11} />
            {runtime.errors}
          </span>
        )}
      </div>
      {isWatched && (
        <div className={styles.watch} title={runtime?.watch ? `Last values: ${runtime.watch}` : "Build, run the package and open Runtime to see the values"}>
          <Eye size={12} />
          <span>{runtime?.watch ?? "Watching, no output yet"}</span>
        </div>
      )}

      <div className={styles.body}>
        <div className={styles.column}>
          {definition.inputs.map((pin) => {
            const connected = editor.connectedPins.has(pinKey(id, "input", pin.name));
            return (
              <PinRow
                key={pin.name}
                nodeId={id}
                pin={pin}
                direction="input"
                diagnostics={pinDiagnostics(pin.name)}
                literal={
                  !connected && (
                    <LiteralEditor
                      catalog={editor.catalog}
                      pin={{ ...pin, type: editor.pinType(id, pin) }}
                      value={data.literals[pin.name]}
                      onChange={(value) => editor.setLiteral(id, pin.name, value)}
                    />
                  )
                }
              />
            );
          })}
        </div>
        <div className={`${styles.column} ${styles.outputs}`}>
          {definition.outputs.map((pin) => {
            const caseId = definition.dynamic?.kind === "cases" && pin.name.startsWith(CASE_PREFIX) ? pin.name.slice(CASE_PREFIX.length) : null;
            const cases = caseId ? switchCases(data.properties) : [];
            const entry = cases.find((candidate) => candidate.id === caseId);
            const numeric = definition.inputs.some(
              (input) => input.name === "value" && input.type.kind === "data" && input.type.type.kind === "named" && input.type.type.of === "integer",
            );
            return (
              <PinRow
                key={pin.name}
                nodeId={id}
                pin={entry ? { ...pin, label: "" } : pin}
                direction="output"
                diagnostics={pinDiagnostics(pin.name)}
                literal={
                  entry && (
                    <CaseEditor
                      entry={entry}
                      numeric={numeric}
                      onChange={(value) =>
                        editor.setProperties(id, {
                          [CASES_PROPERTY]: cases.map((candidate) => (candidate.id === entry.id ? { ...candidate, value } : candidate)),
                        })
                      }
                      onRemove={() =>
                        editor.setProperties(id, { [CASES_PROPERTY]: cases.filter((candidate) => candidate.id !== entry.id) })
                      }
                    />
                  )
                }
              />
            );
          })}
        </div>
      </div>

      <DynamicBar id={id} definition={definition} properties={data.properties} />

      {selected && nodeLevel.length > 0 && (
        <div className={styles.nodeMessages}>
          {nodeLevel.map((diagnostic, index) => (
            <p key={index} data-severity={diagnostic.severity}>
              {diagnostic.message}
            </p>
          ))}
        </div>
      )}
    </div>
  );
});
