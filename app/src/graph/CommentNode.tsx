import { NodeResizer, type NodeProps } from "@xyflow/react";
import { memo, useEffect, useRef, useState } from "react";
import type { FlowNode } from "./convert";
import { useEditor } from "./EditorContext";
import styles from "./CommentNode.module.css";

export const COMMENT_COLORS = ["slate", "blue", "green", "amber", "red", "violet"] as const;

export const CommentNode = memo(function CommentNode({ id, data, selected }: NodeProps<FlowNode>) {
  const editor = useEditor();
  const text = typeof data.properties.text === "string" ? data.properties.text : "Comment";
  const color = typeof data.properties.color === "string" ? data.properties.color : "slate";
  const [editing, setEditing] = useState(editor.editingComment === id);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editor.editingComment === id) {
      setEditing(true);
    }
  }, [editor.editingComment, id]);

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editing]);

  const commit = (value: string) => {
    setEditing(false);
    editor.finishCommentEditing();
    const trimmed = value.trim();
    if (trimmed && trimmed !== text) {
      editor.setProperties(id, { text: trimmed });
    }
  };

  return (
    <div className={styles.comment} data-color={color} data-selected={selected}>
      <NodeResizer
        isVisible={selected}
        minWidth={160}
        minHeight={80}
        lineClassName={styles.resizeLine}
        handleClassName={styles.resizeHandle}
      />
      <div className={styles.header} data-comment-handle onDoubleClick={() => setEditing(true)} title="Double-click to rename">
        {editing ? (
          <input
            ref={inputRef}
            className={`${styles.input} nodrag`}
            defaultValue={text}
            aria-label="Comment text"
            onBlur={(event) => commit(event.target.value)}
            onKeyDown={(event) => {
              event.stopPropagation();
              if (event.key === "Enter") {
                commit(event.currentTarget.value);
              } else if (event.key === "Escape") {
                setEditing(false);
                editor.finishCommentEditing();
              }
            }}
          />
        ) : (
          <span className={styles.text}>{text}</span>
        )}
        {selected && !editing && (
          <span className={`${styles.colors} nodrag`}>
            {COMMENT_COLORS.map((option) => (
              <button
                key={option}
                type="button"
                className={styles.swatch}
                data-color={option}
                aria-label={`${option} color`}
                aria-pressed={option === color}
                onClick={() => editor.setProperties(id, { color: option })}
              />
            ))}
          </span>
        )}
      </div>
    </div>
  );
});
