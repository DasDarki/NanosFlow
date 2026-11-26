import { open as openFile } from "@tauri-apps/plugin-dialog";
import { Activity, ClipboardPaste, Eye, FileText, FolderOpen, Pause, Play, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { useFloating } from "../components/useFloating";
import type { CommandError, LogEntry, LogLocation, ProjectView } from "../types";
import styles from "./RuntimePanel.module.css";

const POLL_INTERVAL = 1000;

type Filter = "all" | "errors" | "watch";

interface RuntimePanelProps {
  project: ProjectView;
  debuggerError: string | null;
  open: boolean;
  entries: LogEntry[];
  onEntries: (entries: LogEntry[], replace: boolean) => void;
  onOpenLocation: (graphId: string, nodeId: string) => void;
  onClose: () => void;
}

function storageKey(projectFile: string): string {
  return `nanosflow.runtime-log:${projectFile}`;
}

function rememberedLog(projectFile: string): string | null {
  try {
    return window.localStorage.getItem(storageKey(projectFile));
  } catch {
    return null;
  }
}

function rememberLog(projectFile: string, path: string) {
  try {
    window.localStorage.setItem(storageKey(projectFile), path);
  } catch {
    return;
  }
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function RuntimePanel({ project, debuggerError, open, entries, onEntries, onOpenLocation, onClose }: RuntimePanelProps) {
  const [logPath, setLogPath] = useState<string | null>(() => rememberedLog(project.projectFile));
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [following, setFollowing] = useState(false);
  const [error, setError] = useState<CommandError | null>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [pasting, setPasting] = useState(false);
  const [pasted, setPasted] = useState("");
  const offsetRef = useRef<number | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const stickRef = useRef(true);
  const panelRef = useRef<HTMLDivElement>(null);
  const floating = useFloating<HTMLDivElement>("runtime", panelRef);

  const graphNames = useMemo(
    () => new Map(project.packages.flatMap((pkg) => pkg.graphs.map((graph) => [graph.id.toLowerCase(), graph.name] as const))),
    [project.packages],
  );
  const graphName = (id: string | undefined) => (id ? graphNames.get(id.toLowerCase()) : undefined);

  useEffect(() => {
    if (!open) {
      return;
    }
    panelRef.current?.focus();
    void api.suggestLogFiles(project.projectFile).then(setSuggestions, () => setSuggestions([]));
  }, [open, project.projectFile]);

  const poll = useCallback(async () => {
    if (!logPath) {
      return;
    }
    try {
      const chunk = await api.readRuntimeLog(project.projectFile, logPath, offsetRef.current);
      const isFirst = offsetRef.current === null;
      offsetRef.current = chunk.offset;
      setError(null);
      if (chunk.entries.length > 0 || isFirst || chunk.restarted) {
        onEntries(chunk.entries, isFirst || chunk.restarted);
      }
    } catch (failure) {
      setError(toCommandError(failure));
      setFollowing(false);
    }
  }, [logPath, project.projectFile, onEntries]);

  useEffect(() => {
    if (!following || !logPath) {
      return;
    }
    void poll();
    const timer = window.setInterval(() => void poll(), POLL_INTERVAL);
    return () => window.clearInterval(timer);
  }, [following, logPath, poll]);

  const follow = (path: string) => {
    offsetRef.current = null;
    setLogPath(path);
    rememberLog(project.projectFile, path);
    setFollowing(true);
    setPasting(false);
  };

  const browse = async () => {
    const picked = await openFile({
      multiple: false,
      directory: false,
      title: "Pick the nanos world log file to follow",
      filters: [{ name: "Log files", extensions: ["log", "txt"] }],
    });
    if (typeof picked === "string") {
      follow(picked);
    }
  };

  const readPasted = async () => {
    try {
      const parsed = await api.parseRuntimeLog(project.projectFile, pasted);
      setFollowing(false);
      onEntries(parsed, true);
      setPasting(false);
      setPasted("");
    } catch (failure) {
      setError(toCommandError(failure));
    }
  };

  const visible = entries.filter((entry) =>
    filter === "errors" ? entry.severity === "error" : filter === "watch" ? entry.severity === "watch" : true,
  );
  const errorCount = entries.filter((entry) => entry.severity === "error").length;
  const watchCount = entries.filter((entry) => entry.severity === "watch").length;

  useEffect(() => {
    const list = listRef.current;
    if (list && stickRef.current) {
      list.scrollTop = list.scrollHeight;
    }
  }, [visible.length, open]);

  if (!open) {
    return null;
  }

  const options = logPath && !suggestions.includes(logPath) ? [logPath, ...suggestions] : suggestions;
  const status = error
    ? error.message
    : logPath
      ? following
        ? `Following ${fileName(logPath)}`
        : `Paused on ${fileName(logPath)}`
      : "No log file selected";

  const locationChip = (location: LogLocation, index: number) => {
    const name = graphName(location.graph);
    if (location.node && location.graph && name) {
      return (
        <button
          key={index}
          type="button"
          className={styles.location}
          title={`${location.file}:${location.line}. Open the node that produced this line.`}
          onClick={() => onOpenLocation(location.graph as string, location.node as string)}
        >
          {name}
          <span>line {location.line}</span>
        </button>
      );
    }
    return (
      <span
        key={index}
        className={styles.location}
        data-unmapped
        title="This line is not part of the last NanosFlow build of this project. Build again if the package changed."
      >
        {location.file}:{location.line}
      </span>
    );
  };

  return (
    <div
      ref={panelRef}
      className={styles.panel}
      style={floating.style}
      role="dialog"
      aria-label="Runtime log"
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key === "Escape" && !pasting) {
          onClose();
        }
      }}
    >
      <header className={styles.header} {...floating.handle}>
        <span className={styles.statusIcon} data-live={following && !error}>
          <Activity size={17} />
        </span>
        <div className={styles.titles}>
          <h2>Runtime</h2>
          <p data-error={Boolean(error)}>{status}</p>
        </div>
        {logPath && (
          <Button
            variant="ghost"
            size="small"
            icon={following ? <Pause size={14} /> : <Play size={14} />}
            onClick={() => setFollowing((value) => !value)}
            title={following ? "Pause following the log" : "Resume following the log"}
          />
        )}
        <Button variant="ghost" size="small" icon={<Trash2 size={14} />} onClick={() => onEntries([], true)} title="Clear the list" />
        <Button variant="ghost" size="small" icon={<X size={15} />} onClick={onClose} title="Close (Esc). Following continues in the background." />
      </header>

      <div className={styles.source}>
        <FileText size={14} />
        <select
          className={styles.select}
          value={logPath ?? ""}
          aria-label="Log file"
          onChange={(event) => event.target.value && follow(event.target.value)}
        >
          <option value="" disabled>
            {options.length > 0 ? "Pick a log file" : "No log files found next to the output folder"}
          </option>
          {options.map((path) => (
            <option key={path} value={path} title={path}>
              {fileName(path)}
            </option>
          ))}
        </select>
        <Button variant="ghost" size="small" icon={<FolderOpen size={14} />} onClick={() => void browse()}>
          Browse
        </Button>
        <Button variant="ghost" size="small" icon={<ClipboardPaste size={14} />} onClick={() => setPasting((value) => !value)}>
          Paste
        </Button>
      </div>

      {pasting && (
        <div className={styles.paste}>
          <textarea
            autoFocus
            value={pasted}
            placeholder="Paste console output, for example from the nanos world client console or a shared log"
            spellCheck={false}
            onChange={(event) => setPasted(event.target.value)}
          />
          <div className={styles.pasteActions}>
            <Button variant="ghost" size="small" onClick={() => setPasting(false)}>
              Cancel
            </Button>
            <Button variant="primary" size="small" disabled={pasted.trim() === ""} onClick={() => void readPasted()}>
              Read log
            </Button>
          </div>
        </div>
      )}

      {debuggerError && <p className={styles.notice}>{debuggerError}</p>}

      <div className={styles.filters} role="tablist">
        {(
          [
            ["all", `All ${entries.length}`],
            ["errors", `Errors ${errorCount}`],
            ["watch", `Watched ${watchCount}`],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            type="button"
            role="tab"
            aria-selected={filter === value}
            className={styles.filter}
            data-tone={value}
            onClick={() => setFilter(value)}
          >
            {label}
          </button>
        ))}
      </div>

      <div
        ref={listRef}
        className={styles.list}
        onScroll={(event) => {
          const list = event.currentTarget;
          stickRef.current = list.scrollHeight - list.scrollTop - list.clientHeight < 24;
        }}
      >
        {visible.length === 0 ? (
          <div className={styles.empty}>
            {entries.length > 0 ? (
              "Nothing matches this filter."
            ) : (
              <>
                <strong>Follow a log to see what your package does at runtime.</strong>
                <span>
                  Lua errors link to the node that caused them. Right-click a node and pick <em>Watch values</em> to print
                  its values whenever it runs. Build once after changing graphs so lines map to the right nodes.
                </span>
              </>
            )}
          </div>
        ) : (
          visible.map((entry, index) => {
            const [first, ...rest] = entry.text.split("\n");
            return (
              <div key={index} className={styles.entry} data-severity={entry.severity}>
                {entry.watch ? (
                  <div className={styles.watchLine}>
                    <Eye size={13} />
                    <button
                      type="button"
                      className={styles.location}
                      onClick={() => onOpenLocation(entry.watch!.graph, entry.watch!.node)}
                      title="Open the watched node"
                    >
                      {graphName(entry.watch.graph) ?? "Unknown graph"}
                    </button>
                    <code>{entry.watch.values || "ran"}</code>
                  </div>
                ) : (
                  <>
                    <code className={styles.text}>{first}</code>
                    {rest.length > 0 && (
                      <details className={styles.trace}>
                        <summary>Stack traceback, {rest.length} lines</summary>
                        <pre>{rest.join("\n")}</pre>
                      </details>
                    )}
                    {entry.locations.length > 0 && <div className={styles.locations}>{entry.locations.map(locationChip)}</div>}
                  </>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
