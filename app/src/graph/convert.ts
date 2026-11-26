import type { Edge, Node } from "@xyflow/react";
import type { NodeCatalog } from "./catalog";
import { isLink, type LiteralValue, type NodeDocument, type PinLink, type PinValue } from "./model";

export interface FlowNodeData extends Record<string, unknown> {
  nodeType: string;
  literals: Record<string, LiteralValue>;
  properties: Record<string, unknown>;
  staleInputs: Record<string, PinValue>;
  staleExec: Record<string, PinLink>;
}

export interface FlowEdgeData extends Record<string, unknown> {
  exec: boolean;
}

export type FlowNodeType = "flow" | "comment" | "reroute";
export type FlowNode = Node<FlowNodeData, FlowNodeType>;

export const COMMENT = "comment";
export const REROUTE = "flow/reroute";
export const EXEC_REROUTE = "flow/reroute-exec";
export const COMMENT_SIZE = { width: 420, height: 260 };
export const COMMENT_HANDLE = "[data-comment-handle]";
const COMMENT_LAYER = -2000;

export function flowTypeOf(nodeType: string): FlowNodeType {
  if (nodeType === COMMENT) {
    return "comment";
  }
  return nodeType === REROUTE || nodeType === EXEC_REROUTE ? "reroute" : "flow";
}

function numberProperty(properties: Record<string, unknown>, key: string, fallback: number): number {
  const value = properties[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export function shapeNode(node: FlowNode): FlowNode {
  if (node.type !== "comment") {
    return node;
  }
  return {
    ...node,
    zIndex: COMMENT_LAYER,
    dragHandle: COMMENT_HANDLE,
    width: node.width ?? numberProperty(node.data.properties, "width", COMMENT_SIZE.width),
    height: node.height ?? numberProperty(node.data.properties, "height", COMMENT_SIZE.height),
  };
}
export type FlowEdge = Edge<FlowEdgeData, "wire">;

export function dataEdgeId(target: string, targetHandle: string): string {
  return `data:${target}:${targetHandle}`;
}

export function execEdgeId(source: string, sourceHandle: string): string {
  return `exec:${source}:${sourceHandle}`;
}

export function makeEdge(source: string, sourceHandle: string, target: string, targetHandle: string, exec: boolean): FlowEdge {
  return {
    id: exec ? execEdgeId(source, sourceHandle) : dataEdgeId(target, targetHandle),
    type: "wire",
    source,
    sourceHandle,
    target,
    targetHandle,
    data: { exec },
  };
}

export function documentsToFlow(documents: NodeDocument[], catalog: NodeCatalog): { nodes: FlowNode[]; edges: FlowEdge[] } {
  const byId = new Map(documents.map((document) => [document.id, document]));
  const nodes: FlowNode[] = [];
  const edges: FlowEdge[] = [];

  for (const document of documents) {
    const definition = catalog.definitionFor(document.type, document.properties);
    const literals: Record<string, LiteralValue> = {};
    const staleInputs: Record<string, PinValue> = {};
    const staleExec: Record<string, PinLink> = {};

    for (const [pinName, value] of Object.entries(document.inputs ?? {})) {
      const pin = definition?.inputs.find((candidate) => candidate.name === pinName);
      if (!pin || pin.type.kind === "exec") {
        staleInputs[pinName] = value;
        continue;
      }
      if (!isLink(value)) {
        literals[pinName] = value.literal;
        continue;
      }
      const source = byId.get(value.node);
      const sourcePin = source && catalog.definitionFor(source.type, source.properties)?.outputs.find((candidate) => candidate.name === value.pin);
      if (!sourcePin || sourcePin.type.kind === "exec") {
        staleInputs[pinName] = value;
        continue;
      }
      edges.push(makeEdge(value.node, value.pin, document.id, pinName, false));
    }

    for (const [pinName, link] of Object.entries(document.exec ?? {})) {
      const pin = definition?.outputs.find((candidate) => candidate.name === pinName);
      const target = byId.get(link.node);
      const targetPin = target && catalog.definitionFor(target.type, target.properties)?.inputs.find((candidate) => candidate.name === link.pin);
      if (!pin || pin.type.kind !== "exec" || !targetPin || targetPin.type.kind !== "exec") {
        staleExec[pinName] = link;
        continue;
      }
      edges.push(makeEdge(document.id, pinName, link.node, link.pin, true));
    }

    nodes.push(shapeNode({
      id: document.id,
      type: flowTypeOf(document.type),
      position: { x: document.position[0], y: document.position[1] },
      data: {
        nodeType: document.type,
        literals,
        properties: document.properties ?? {},
        staleInputs,
        staleExec,
      },
    }));
  }
  return { nodes, edges };
}

function isEmpty(record: Record<string, unknown>): boolean {
  return Object.keys(record).length === 0;
}

export function flowToDocuments(nodes: FlowNode[], edges: FlowEdge[]): NodeDocument[] {
  const inputs = new Map<string, Record<string, PinValue>>();
  const exec = new Map<string, Record<string, PinLink>>();

  for (const node of nodes) {
    const nodeInputs: Record<string, PinValue> = { ...node.data.staleInputs };
    for (const [pinName, literal] of Object.entries(node.data.literals)) {
      nodeInputs[pinName] = { literal };
    }
    inputs.set(node.id, nodeInputs);
    exec.set(node.id, { ...node.data.staleExec });
  }

  for (const edge of edges) {
    if (!edge.sourceHandle || !edge.targetHandle) {
      continue;
    }
    if (edge.data?.exec) {
      const record = exec.get(edge.source);
      if (record) {
        record[edge.sourceHandle] = { node: edge.target, pin: edge.targetHandle };
      }
    } else {
      const record = inputs.get(edge.target);
      if (record) {
        record[edge.targetHandle] = { node: edge.source, pin: edge.sourceHandle };
      }
    }
  }

  return nodes.map((node) => {
    const document: NodeDocument = {
      id: node.id,
      type: node.data.nodeType,
      position: [Math.round(node.position.x), Math.round(node.position.y)],
    };
    const nodeInputs = inputs.get(node.id) ?? {};
    const nodeExec = exec.get(node.id) ?? {};
    if (!isEmpty(nodeInputs)) {
      document.inputs = nodeInputs;
    }
    if (!isEmpty(nodeExec)) {
      document.exec = nodeExec;
    }
    const properties =
      node.type === "comment"
        ? {
            ...node.data.properties,
            width: Math.round(node.width ?? node.measured?.width ?? COMMENT_SIZE.width),
            height: Math.round(node.height ?? node.measured?.height ?? COMMENT_SIZE.height),
          }
        : node.data.properties;
    if (!isEmpty(properties)) {
      document.properties = properties;
    }
    return document;
  });
}

export function serializeDocument(document: NodeDocument): string {
  const sortKeys = (value: unknown): unknown => {
    if (Array.isArray(value) || value === null || typeof value !== "object") {
      return value;
    }
    return Object.fromEntries(
      Object.keys(value as Record<string, unknown>)
        .sort()
        .map((key) => [key, sortKeys((value as Record<string, unknown>)[key])]),
    );
  };
  return JSON.stringify(sortKeys(document));
}
