import type { NodeDefinition, PinDefinition } from "./model";

export const COUNT_PROPERTY = "count";
export const CASES_PROPERTY = "cases";
export const CASE_PREFIX = "case_";
export const TRACKS_PROPERTY = "tracks";
export const LENGTH_PROPERTY = "length";
export const LOOPING_PROPERTY = "looping";
export const TRACK_PREFIX = "track_";
export const DEFAULT_TIMELINE_LENGTH = 1;

export interface TimelineTrack {
  id: string;
  name: string;
  keys: [number, number][];
}

export function timelineTracks(properties: Record<string, unknown>): TimelineTrack[] {
  const raw = properties[TRACKS_PROPERTY];
  if (!Array.isArray(raw)) {
    return [];
  }
  return raw
    .filter(
      (entry): entry is TimelineTrack =>
        typeof entry === "object" && entry !== null && typeof (entry as TimelineTrack).id === "string" && /^[A-Za-z0-9]+$/.test((entry as TimelineTrack).id),
    )
    .map((entry) => ({
      id: entry.id,
      name: typeof entry.name === "string" ? entry.name : "Track",
      keys: (Array.isArray(entry.keys) ? entry.keys : [])
        .filter((key): key is [number, number] => Array.isArray(key) && typeof key[0] === "number" && typeof key[1] === "number")
        .map(([time, value]) => [time, value] as [number, number])
        .sort((left, right) => left[0] - right[0]),
    }));
}

export function timelineLength(properties: Record<string, unknown>): number {
  const length = properties[LENGTH_PROPERTY];
  return typeof length === "number" && length > 0 ? length : DEFAULT_TIMELINE_LENGTH;
}
const MAXIMUM_PINS = 64;
const WILDCARD = { kind: "named", of: "$T" } as const;

export interface SwitchCase {
  id: string;
  value: number | string;
}

export function switchCases(properties: Record<string, unknown>): SwitchCase[] {
  const raw = properties[CASES_PROPERTY];
  if (!Array.isArray(raw)) {
    return [];
  }
  return raw.filter(
    (entry): entry is SwitchCase =>
      typeof entry === "object" &&
      entry !== null &&
      typeof (entry as SwitchCase).id === "string" &&
      /^[A-Za-z0-9]+$/.test((entry as SwitchCase).id) &&
      (typeof (entry as SwitchCase).value === "number" || typeof (entry as SwitchCase).value === "string"),
  );
}

export function pinCount(definition: NodeDefinition, properties: Record<string, unknown>): number {
  const dynamic = definition.dynamic;
  if (!dynamic || dynamic.kind === "cases" || dynamic.kind === "tracks") {
    return 0;
  }
  const stored = properties[COUNT_PROPERTY];
  const count = typeof stored === "number" && Number.isInteger(stored) && stored >= 0 ? stored : dynamic.fallback;
  return Math.min(Math.max(count, dynamic.minimum), MAXIMUM_PINS);
}

function caseLabel(value: number | string): string {
  return typeof value === "string" ? `"${value}"` : String(value);
}

export function expandDefinition(definition: NodeDefinition, properties: Record<string, unknown>): NodeDefinition {
  const dynamic = definition.dynamic;
  if (!dynamic) {
    return definition;
  }
  switch (dynamic.kind) {
    case "exec-outputs": {
      const pins: PinDefinition[] = Array.from({ length: pinCount(definition, properties) }, (_, index) => ({
        name: `${dynamic.prefix}${index}`,
        label: `${dynamic.label} ${index}`,
        type: { kind: "exec" },
        optional: true,
      }));
      return { ...definition, outputs: [...pins, ...definition.outputs] };
    }
    case "data-inputs": {
      const pins: PinDefinition[] = Array.from({ length: pinCount(definition, properties) }, (_, index) => ({
        name: `${dynamic.prefix}${index}`,
        label: `${dynamic.label} ${index}`,
        type: { kind: "data", type: WILDCARD },
        optional: true,
      }));
      return { ...definition, inputs: [...definition.inputs, ...pins] };
    }
    case "tracks": {
      const pins: PinDefinition[] = timelineTracks(properties).map((track) => ({
        name: `${TRACK_PREFIX}${track.id}`,
        label: track.name,
        type: { kind: "data", type: { kind: "named", of: "number" } },
        optional: true,
      }));
      return { ...definition, outputs: [...definition.outputs, ...pins] };
    }
    case "cases": {
      const pins: PinDefinition[] = switchCases(properties).map((entry) => ({
        name: `${CASE_PREFIX}${entry.id}`,
        label: caseLabel(entry.value),
        type: { kind: "exec" },
        optional: true,
      }));
      return { ...definition, outputs: [...pins, ...definition.outputs] };
    }
  }
}

export function initialProperties(definition: NodeDefinition): Record<string, unknown> {
  const dynamic = definition.dynamic;
  if (!dynamic) {
    return {};
  }
  if (dynamic.kind === "tracks") {
    return {
      [LENGTH_PROPERTY]: DEFAULT_TIMELINE_LENGTH,
      [TRACKS_PROPERTY]: [{ id: "alpha", name: "Alpha", keys: [[0, 0], [DEFAULT_TIMELINE_LENGTH, 1]] }],
    };
  }
  return dynamic.kind === "cases" ? { [CASES_PROPERTY]: [] } : { [COUNT_PROPERTY]: dynamic.initial };
}
