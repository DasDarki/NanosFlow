import { ArrowDownToLine, ArrowUpFromLine, CirclePause, Play, Redo2 } from "lucide-react";
import { useEffect } from "react";
import { Button } from "../components/Button";
import type { BreakpointHit, ResumeMode, StackFrame } from "../types";
import styles from "./DebugBar.module.css";

interface DebugBarProps {
  hit: BreakpointHit;
  location: string;
  onResume: (mode: ResumeMode) => void;
  onReveal: () => void;
  describeFrame: (frame: StackFrame) => string;
  onRevealFrame: (frame: StackFrame) => void;
}

export function DebugBar({ hit, location, onResume, onReveal, describeFrame, onRevealFrame }: DebugBarProps) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const mode: ResumeMode | null =
        event.key === "F8" || event.key === "F5"
          ? "continue"
          : event.key === "F10"
            ? "over"
            : event.key === "F11"
              ? event.shiftKey
                ? "out"
                : "into"
              : null;
      if (mode) {
        event.preventDefault();
        onResume(mode);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onResume]);

  const frames = [...hit.stack].reverse();

  return (
    <div className={styles.bar} role="status" aria-live="assertive">
      <div className={styles.header}>
        <CirclePause size={18} className={styles.icon} />
        <div className={styles.titles}>
          <strong>Paused on the {hit.side || "server"}</strong>
          <button type="button" className={styles.location} onClick={onReveal} title="Show the node">
            {location}
          </button>
        </div>
        <Button
          variant="ghost"
          size="small"
          icon={<ArrowDownToLine size={14} />}
          onClick={() => onResume("into")}
          title="Step into: pause at the next node that runs, also inside functions it calls (F11)"
        >
          Into
        </Button>
        <Button
          variant="ghost"
          size="small"
          icon={<Redo2 size={14} />}
          onClick={() => onResume("over")}
          title="Step over: run this node completely and pause at the next node of this graph (F10)"
        >
          Over
        </Button>
        <Button
          variant="ghost"
          size="small"
          icon={<ArrowUpFromLine size={14} />}
          disabled={hit.stack.length < 2}
          onClick={() => onResume("out")}
          title="Step out: finish this function and pause where it was called (Shift+F11)"
        >
          Out
        </Button>
        <Button variant="primary" size="small" icon={<Play size={14} />} onClick={() => onResume("continue")} title="Continue running (F8)">
          Continue
        </Button>
      </div>
      {hit.values.length > 0 ? (
        <dl className={styles.values}>
          {hit.values.map(([name, value], index) => (
            <div key={`${name}:${index}`} className={styles.value}>
              <dt>{name}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className={styles.hint}>This node has no inputs to show.</p>
      )}
      {frames.length > 0 && (
        <div className={styles.stack}>
          <span className={styles.stackTitle}>Call stack</span>
          <ol>
            {frames.map((frame, index) => (
              <li key={`${frame.location}:${index}`}>
                <button
                  type="button"
                  disabled={!frame.graph || !frame.node}
                  onClick={() => onRevealFrame(frame)}
                  title={index === 0 ? "Where the paused code started in this graph" : "Where this call came from"}
                >
                  {describeFrame(frame)}
                </button>
              </li>
            ))}
          </ol>
        </div>
      )}
      <p className={styles.hint}>The whole server waits until you continue. Players may time out if it stays paused for long.</p>
    </div>
  );
}
