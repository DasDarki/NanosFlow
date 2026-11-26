import type { NodeCatalog } from "./catalog";
import type { FlowEdge, FlowNode } from "./convert";
import type { PinDefinition, PinType, TypeExpr } from "./model";

export const TYPE_PARAMETER = "$T";
const ANY: TypeExpr = { kind: "named", of: "any" };
const MAX_DEPTH = 32;

export function containsTypeParameter(type: TypeExpr): boolean {
  switch (type.kind) {
    case "named":
      return type.of === TYPE_PARAMETER;
    case "union":
      return type.of.some(containsTypeParameter);
    default:
      return containsTypeParameter(type.of);
  }
}

export function substitute(type: TypeExpr, binding: TypeExpr | undefined): TypeExpr {
  switch (type.kind) {
    case "named":
      return type.of === TYPE_PARAMETER ? (binding ?? ANY) : type;
    case "union":
      return { kind: "union", of: type.of.map((member) => substitute(member, binding)) };
    default:
      return { kind: type.kind, of: substitute(type.of, binding) };
  }
}

function inferBinding(declared: TypeExpr, actual: TypeExpr): TypeExpr | undefined {
  let binding: TypeExpr | undefined;
  if (declared.kind === "named" && declared.of === TYPE_PARAMETER) {
    binding = actual;
  } else if (declared.kind === "array" && actual.kind === "array") {
    binding = inferBinding(declared.of, actual.of);
  } else if (declared.kind === "optional") {
    binding = inferBinding(declared.of, actual.kind === "optional" ? actual.of : actual);
  }
  return binding && !(binding.kind === "named" && binding.of === "any") ? binding : undefined;
}

export class TypeResolver {
  private readonly nodes: Map<string, FlowNode>;
  private readonly incoming = new Map<string, FlowEdge>();

  constructor(
    private readonly catalog: NodeCatalog,
    nodes: FlowNode[],
    edges: FlowEdge[],
  ) {
    this.nodes = new Map(nodes.map((node) => [node.id, node]));
    for (const edge of edges) {
      if (edge.targetHandle) {
        this.incoming.set(`${edge.target}:${edge.targetHandle}`, edge);
      }
    }
  }

  pinType(nodeId: string, pin: PinDefinition, depth = 0): PinType {
    if (pin.type.kind === "exec" || !containsTypeParameter(pin.type.type)) {
      return pin.type;
    }
    return { kind: "data", type: substitute(pin.type.type, this.binding(nodeId, depth)) };
  }

  private binding(nodeId: string, depth: number): TypeExpr | undefined {
    if (depth > MAX_DEPTH) {
      return undefined;
    }
    const node = this.nodes.get(nodeId);
    const definition = node ? this.catalog.definitionFor(node.data.nodeType, node.data.properties) : undefined;
    if (!definition) {
      return undefined;
    }
    const pins = definition.inputs
      .filter((pin) => pin.type.kind === "data" && containsTypeParameter(pin.type.type))
      .sort((left, right) => Number(right.type.kind === "data" && right.type.type.kind === "array") - Number(left.type.kind === "data" && left.type.type.kind === "array"));
    for (const pin of pins) {
      const edge = this.incoming.get(`${nodeId}:${pin.name}`);
      const source = edge ? this.nodes.get(edge.source) : undefined;
      const output = source ? this.catalog.definitionFor(source.data.nodeType, source.data.properties)?.outputs.find((candidate) => candidate.name === edge?.sourceHandle) : undefined;
      if (!edge || !output || pin.type.kind !== "data") {
        continue;
      }
      const actual = this.pinType(edge.source, output, depth + 1);
      if (actual.kind === "data") {
        const binding = inferBinding(pin.type.type, actual.type);
        if (binding) {
          return binding;
        }
      }
    }
    return undefined;
  }
}
