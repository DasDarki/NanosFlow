import type { NodeCatalog, TypeCategory } from "./catalog";
import { baseType } from "./catalog";
import type { NodeDefinition, NodeKind, PinType } from "./model";

const EXEC_COLOR = "#f1f1f6";

const CATEGORY_COLORS: Record<TypeCategory, string> = {
  boolean: "#f25f7a",
  number: "#86e08f",
  integer: "#3fd6b8",
  string: "#f47ad6",
  "asset-path": "#f9a8d4",
  table: "#9aa6bb",
  function: "#fb9a4b",
  any: "#c9c9d8",
  enum: "#b6e34f",
  struct: "#f5c84b",
  object: "#5fa8ff",
};

const STRUCT_COLORS: Record<string, string> = {
  Vector: "#f5c84b",
  Vector2D: "#f0a94a",
  Rotator: "#a99bff",
  Color: "#4cc9f0",
  Quat: "#8b9dff",
  Matrix: "#c4b5fd",
};

export function pinColor(catalog: NodeCatalog, type: PinType): string {
  if (type.kind === "exec") {
    return EXEC_COLOR;
  }
  const base = baseType(type.type);
  const named = base.kind === "array" ? baseType(base.of) : base;
  if (named.kind === "named" && STRUCT_COLORS[named.of]) {
    return STRUCT_COLORS[named.of];
  }
  return CATEGORY_COLORS[catalog.typeCategory(base.kind === "array" ? base.of : base)];
}

export function isArrayPin(type: PinType): boolean {
  return type.kind === "data" && baseType(type.type).kind === "array";
}

export const KIND_COLORS: Record<NodeKind, string> = {
  event: "#d6334a",
  function: "#3a73e8",
  "pure-function": "#1f9d5c",
  constructor: "#8a4de8",
  "flow-control": "#5d5d70",
  operator: "#14917f",
  literal: "#6b6478",
  "function-entry": "#9b4dca",
  "function-return": "#9b4dca",
  variable: "#2f7f8f",
  comment: "rgba(255, 255, 255, 0.07)",
  reroute: "#8a8aa0",
};

export const MACRO_COLOR = "#1c8aa0";

export function definitionColor(definition: Pick<NodeDefinition, "id" | "kind">): string {
  return definition.id.startsWith("macro/") ? MACRO_COLOR : KIND_COLORS[definition.kind];
}

export const KIND_LABELS: Record<NodeKind, string> = {
  event: "Event",
  function: "Function",
  "pure-function": "Pure function",
  constructor: "Spawn",
  "flow-control": "Flow control",
  operator: "Operator",
  literal: "Literal",
  "function-entry": "Function entry",
  "function-return": "Function return",
  variable: "Variable",
  comment: "Comment",
  reroute: "Reroute",
};
