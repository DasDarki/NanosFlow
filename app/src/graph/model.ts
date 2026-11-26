import type { ApiChannel, EventDirection, GraphKind } from "../types";

export type TypeExpr =
  | { kind: "named"; of: string }
  | { kind: "array"; of: TypeExpr }
  | { kind: "optional"; of: TypeExpr }
  | { kind: "variadic"; of: TypeExpr }
  | { kind: "union"; of: TypeExpr[] };

export type PinType = { kind: "exec" } | { kind: "data"; type: TypeExpr };

export interface PinDefinition {
  name: string;
  label: string;
  type: PinType;
  description?: string;
  default?: string;
  optional: boolean;
}

export type NodeKind =
  | "event"
  | "function"
  | "pure-function"
  | "constructor"
  | "flow-control"
  | "operator"
  | "literal"
  | "function-entry"
  | "function-return"
  | "variable"
  | "comment"
  | "reroute";
export type Side = "both" | "server" | "client";

export type DynamicPins =
  | { kind: "exec-outputs" | "data-inputs"; prefix: string; label: string; minimum: number; initial: number; fallback: number }
  | { kind: "cases" }
  | { kind: "tracks" };

export interface NodeDefinition {
  id: string;
  title: string;
  subtitle?: string;
  category: string;
  kind: NodeKind;
  side: Side;
  description?: string;
  inputs: PinDefinition[];
  outputs: PinDefinition[];
  keywords?: string[];
  dynamic?: DynamicPins;
}

export interface ClassInfo {
  name: string;
  kind: "class" | "static-class" | "standard-library" | "struct" | "utility-class";
  parents: string[];
  isBase: boolean;
  inheritable: boolean;
}

export interface EnumInfo {
  name: string;
  values: { key: string; value: string; label?: string; description?: string }[];
}

export interface CatalogPayload {
  channel: ApiChannel;
  revision: string;
  nodes: NodeDefinition[];
  classes: ClassInfo[];
  enums: EnumInfo[];
}

export type LiteralValue = boolean | number | string | Record<string, number>;

export interface PinLink {
  node: string;
  pin: string;
}

export type PinValue = PinLink | { literal: LiteralValue };

export interface NodeDocument {
  id: string;
  type: string;
  position: [number, number];
  inputs?: Record<string, PinValue>;
  exec?: Record<string, PinLink>;
  properties?: Record<string, unknown>;
}

export interface FunctionPin {
  id: string;
  name: string;
  type: string;
}

export interface FunctionSignature {
  description?: string;
  pure?: boolean;
  exported?: boolean;
  parameters?: FunctionPin[];
  returns?: FunctionPin[];
}

export interface GraphManifest extends FunctionSignature {
  id: string;
  kind: GraphKind;
  parent?: string;
  interfaces?: string[];
  functions?: InterfaceFunction[];
  direction?: EventDirection;
}

export interface VariableDocument {
  id: string;
  name: string;
  type: string;
  default?: LiteralValue;
  description?: string;
  sync?: boolean;
}

export interface ProjectClass {
  id: string;
  name: string;
  parent: string;
  parents: string[];
  apiRoot?: string | null;
}

export interface ProjectStruct {
  id: string;
  name: string;
  fields: FunctionPin[];
}

export interface ProjectEnum {
  id: string;
  name: string;
  values: FunctionPin[];
}

export type ConflictKind = "node" | "variable" | "graph";

export type ConflictSide =
  | { type: "node"; node: NodeDocument }
  | { type: "variable"; variable: VariableDocument }
  | { type: "graph"; manifest: GraphManifest }
  | { type: "deleted" }
  | { type: "unreadable"; text: string; error: string };

export interface FileConflict {
  file: string;
  kind: ConflictKind;
  oursLabel: string;
  theirsLabel: string;
  ours: ConflictSide;
  theirs: ConflictSide;
}

export type ConflictChoice = "ours" | "theirs";

export interface CommittedGraph {
  revision: string;
  summary: string;
  nodes: NodeDocument[];
}

export interface ScopeData {
  definitions: NodeDefinition[];
  classes: ProjectClass[];
  structs: ProjectStruct[];
  enums: ProjectEnum[];
  interfaces: string[];
}

export interface InterfaceFunction {
  id: string;
  name: string;
  description?: string;
  parameters?: FunctionPin[];
  returns?: FunctionPin[];
}

export interface NodeLoadError {
  file: string;
  message: string;
  hasConflictMarkers: boolean;
}

export interface GraphDocument {
  manifest: GraphManifest;
  nodes: NodeDocument[];
  variables: VariableDocument[];
  loadErrors: NodeLoadError[];
}

export type DiagnosticSeverity = "error" | "warning";

export interface Diagnostic {
  severity: DiagnosticSeverity;
  code: string;
  node?: string;
  pin?: string;
  message: string;
  hint?: string;
}

export function isLink(value: PinValue): value is PinLink {
  return "node" in value;
}
