import { ChevronDown, ChevronUp, CircleAlert, CircleCheck, FileWarning, TriangleAlert } from "lucide-react";
import { useState } from "react";
import type { Diagnostic, NodeLoadError } from "./model";
import styles from "./ProblemsPanel.module.css";

interface ProblemsPanelProps {
  diagnostics: Diagnostic[];
  loadErrors: NodeLoadError[];
  onSelect: (diagnostic: Diagnostic) => void;
}

export function ProblemsPanel({ diagnostics, loadErrors, onSelect }: ProblemsPanelProps) {
  const [open, setOpen] = useState(false);
  const errorCount = diagnostics.filter((diagnostic) => diagnostic.severity === "error").length + loadErrors.length;
  const warningCount = diagnostics.filter((diagnostic) => diagnostic.severity === "warning").length;
  const total = errorCount + warningCount;
  const sorted = [...diagnostics].sort((left, right) => (left.severity === right.severity ? 0 : left.severity === "error" ? -1 : 1));

  return (
    <section className={styles.panel} data-open={open}>
      <button type="button" className={styles.header} onClick={() => setOpen((value) => !value)} aria-expanded={open}>
        <span className={styles.heading}>Problems</span>
        {total === 0 ? (
          <span className={styles.clean}>
            <CircleCheck size={14} /> No problems
          </span>
        ) : (
          <>
            {errorCount > 0 && (
              <span className={styles.count} data-severity="error">
                <CircleAlert size={14} /> {errorCount} {errorCount === 1 ? "error" : "errors"}
              </span>
            )}
            {warningCount > 0 && (
              <span className={styles.count} data-severity="warning">
                <TriangleAlert size={14} /> {warningCount} {warningCount === 1 ? "warning" : "warnings"}
              </span>
            )}
          </>
        )}
        <span className={styles.toggle}>{open ? <ChevronDown size={15} /> : <ChevronUp size={15} />}</span>
      </button>
      {open && (
        <div className={styles.list}>
          {total === 0 && <p className={styles.empty}>Everything in this graph checks out.</p>}
          {loadErrors.map((error) => (
            <div key={error.file} className={styles.item} data-severity="error">
              <FileWarning size={15} />
              <div>
                <p className={styles.message}>
                  <code>nodes/{error.file}</code>: {error.message}
                </p>
              </div>
            </div>
          ))}
          {sorted.map((diagnostic, index) => (
            <button
              key={`${diagnostic.node}-${diagnostic.code}-${diagnostic.pin}-${index}`}
              type="button"
              className={styles.item}
              data-severity={diagnostic.severity}
              onClick={() => onSelect(diagnostic)}
              title="Show the node"
            >
              {diagnostic.severity === "error" ? <CircleAlert size={15} /> : <TriangleAlert size={15} />}
              <div>
                <p className={styles.message}>{diagnostic.message}</p>
                {diagnostic.hint && <p className={styles.hint}>{diagnostic.hint}</p>}
              </div>
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
