import { serializeDocument } from "./convert";
import type { NodeDocument } from "./model";

export type ChangeKind = "added" | "changed" | "moved";

export interface GraphChanges {
  byNode: Record<string, ChangeKind>;
  removed: NodeDocument[];
  committed: Map<string, NodeDocument>;
}

function compact(record: Record<string, unknown> | undefined): Record<string, unknown> | undefined {
  if (!record) {
    return undefined;
  }
  const entries = Object.entries(record).filter(([, value]) => value !== undefined && value !== null);
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

function withoutPosition(document: NodeDocument): string {
  return serializeDocument({
    ...document,
    position: [0, 0],
    inputs: compact(document.inputs) as NodeDocument["inputs"],
    exec: compact(document.exec) as NodeDocument["exec"],
    properties: compact(document.properties),
  });
}

export function computeChanges(current: NodeDocument[], committedNodes: NodeDocument[]): GraphChanges {
  const committed = new Map(committedNodes.map((node) => [node.id, node]));
  const currentIds = new Set(current.map((node) => node.id));
  const byNode: Record<string, ChangeKind> = {};
  for (const node of current) {
    const before = committed.get(node.id);
    if (!before) {
      byNode[node.id] = "added";
    } else if (withoutPosition(before) !== withoutPosition(node)) {
      byNode[node.id] = "changed";
    } else if (before.position[0] !== node.position[0] || before.position[1] !== node.position[1]) {
      byNode[node.id] = "moved";
    }
  }
  const removed = committedNodes.filter((node) => !currentIds.has(node.id));
  return { byNode, removed, committed };
}
