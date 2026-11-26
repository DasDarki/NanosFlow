import { Handle, Position, type NodeProps } from "@xyflow/react";
import { memo } from "react";
import { EXEC_REROUTE, type FlowNode } from "./convert";
import { useEditor } from "./EditorContext";
import styles from "./RerouteNode.module.css";

export const RerouteNode = memo(function RerouteNode({ id, data, selected }: NodeProps<FlowNode>) {
  const editor = useEditor();
  const exec = data.nodeType === EXEC_REROUTE;
  const color = exec ? "#ececf4" : editor.wireColor(id, "value");
  return (
    <div className={styles.knot} data-selected={selected} data-exec={exec} style={{ background: color }} title="Reroute">
      <Handle type="target" position={Position.Left} id={exec ? "exec" : "value"} className={styles.handle} />
      <Handle type="source" position={Position.Right} id={exec ? "then" : "value"} className={styles.handle} />
    </div>
  );
});
