import { createContext, useContext } from "react";
import type { RuntimeMark, ScriptContext } from "../types";
import type { NodeCatalog } from "./catalog";
import type { ChangeKind } from "./changes";
import type { Diagnostic, LiteralValue, PinDefinition, PinType } from "./model";

export interface EditorApi {
  catalog: NodeCatalog;
  context: ScriptContext;
  diagnosticsByNode: Map<string, Diagnostic[]>;
  connectedPins: Set<string>;
  setLiteral: (nodeId: string, pin: string, value: LiteralValue | undefined) => void;
  disconnectPin: (nodeId: string, pin: string, direction: "input" | "output") => void;
  setProperties: (nodeId: string, patch: Record<string, unknown>) => void;
  editTimeline: (nodeId: string) => void;
  wireColor: (nodeId: string, outputPin: string) => string;
  pinType: (nodeId: string, pin: PinDefinition) => PinType;
  editingComment: string | null;
  finishCommentEditing: () => void;
  runtime: Record<string, RuntimeMark>;
  changes: Record<string, ChangeKind>;
  remoteSelection: Record<string, { color: string; name: string }>;
}

export const EditorContext = createContext<EditorApi | null>(null);

export function useEditor(): EditorApi {
  const editor = useContext(EditorContext);
  if (!editor) {
    throw new Error("useEditor must be used inside a graph editor");
  }
  return editor;
}

export function pinKey(nodeId: string, direction: "input" | "output", pin: string): string {
  return `${nodeId}:${direction}:${pin}`;
}
