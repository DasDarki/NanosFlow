import { BaseEdge, getBezierPath, useNodesData, type ConnectionLineComponentProps, type EdgeProps } from "@xyflow/react";
import { memo } from "react";
import type { FlowEdge, FlowNode } from "./convert";
import { useEditor } from "./EditorContext";
import { pinColor } from "./pinStyle";

function useSourcePinColor(source: string, sourceHandle: string | null | undefined): string {
  const editor = useEditor();
  useNodesData<FlowNode>(source);
  return editor.wireColor(source, sourceHandle ?? "");
}

export const Wire = memo(function Wire({
  id,
  source,
  sourceHandleId,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  selected,
  data,
}: EdgeProps<FlowEdge>) {
  const color = useSourcePinColor(source, sourceHandleId);
  const [path] = getBezierPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition, curvature: 0.35 });
  const width = data?.exec ? 3 : 2.25;
  return (
    <>
      {selected && <path d={path} fill="none" stroke={color} strokeOpacity={0.28} strokeWidth={width + 7} />}
      <BaseEdge id={id} path={path} interactionWidth={14} style={{ stroke: color, strokeWidth: width, strokeOpacity: selected ? 1 : 0.85 }} />
    </>
  );
});

export function ConnectionLine({ fromX, fromY, toX, toY, fromPosition, toPosition, fromNode, fromHandle, connectionStatus }: ConnectionLineComponentProps) {
  const editor = useEditor();
  const data = fromNode.data as FlowNode["data"];
  const definition = editor.catalog.definitionFor(data.nodeType, data.properties);
  const pins = fromHandle.type === "source" ? definition?.outputs : definition?.inputs;
  const pin = pins?.find((candidate) => candidate.name === fromHandle.id);
  const color = connectionStatus === "invalid" ? "#f87171" : pin ? pinColor(editor.catalog, pin.type) : "#c9c9d8";
  const [path] = getBezierPath({
    sourceX: fromX,
    sourceY: fromY,
    sourcePosition: fromPosition,
    targetX: toX,
    targetY: toY,
    targetPosition: toPosition,
    curvature: 0.35,
  });
  return (
    <path
      d={path}
      fill="none"
      stroke={color}
      strokeWidth={pin?.type.kind === "exec" ? 3 : 2.25}
      strokeDasharray={connectionStatus === "invalid" ? "6 5" : undefined}
    />
  );
}
