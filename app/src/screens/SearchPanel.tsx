import { Box, CornerDownLeft, LoaderCircle, MessageSquare, Quote, Search, Shapes, Variable, Workflow, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, toCommandError } from "../api";
import type { CommandError, HitKind, ProjectView, SearchHit } from "../types";
import styles from "./SearchPanel.module.css";

export type SearchRequest = { mode: "text"; query: string } | { mode: "usages"; target: string; label: string };

interface SearchPanelProps {
  project: ProjectView;
  request: SearchRequest;
  onRequestChange: (request: SearchRequest) => void;
  onSelect: (hit: SearchHit) => void;
  onClose: () => void;
}

interface SearchState {
  hits: SearchHit[];
  truncated: boolean;
  subject: { name: string; kind: string } | null;
  loading: boolean;
  error: CommandError | null;
}

const SEARCH_DELAY = 180;

const KIND_ICONS: Record<HitKind, ReactNode> = {
  graph: <Workflow size={14} />,
  node: <Box size={14} />,
  comment: <MessageSquare size={14} />,
  literal: <Quote size={14} />,
  variable: <Variable size={14} />,
  type: <Shapes size={14} />,
};

function highlighted(text: string, query: string): ReactNode {
  const needle = query.trim().toLowerCase();
  const index = needle ? text.toLowerCase().indexOf(needle) : -1;
  if (index < 0) {
    return text;
  }
  return (
    <>
      {text.slice(0, index)}
      <mark className={styles.match}>{text.slice(index, index + needle.length)}</mark>
      {text.slice(index + needle.length)}
    </>
  );
}

export function SearchPanel({ project, request, onRequestChange, onSelect, onClose }: SearchPanelProps) {
  const [state, setState] = useState<SearchState>({ hits: [], truncated: false, subject: null, loading: false, error: null });
  const [active, setActive] = useState(0);
  const panelRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const query = request.mode === "text" ? request.query : "";
  const multiplePackages = project.packages.length > 1;

  useEffect(() => {
    let cancelled = false;
    if (request.mode === "text" && request.query.trim() === "") {
      setState({ hits: [], truncated: false, subject: null, loading: false, error: null });
      return;
    }
    setState((current) => ({ ...current, loading: true, error: null }));
    const timer = window.setTimeout(
      async () => {
        try {
          const prefix = (folder: string, hits: SearchHit[]) =>
            multiplePackages ? hits.map((hit) => ({ ...hit, location: `${folder} / ${hit.location}` })) : hits;
          if (request.mode === "text") {
            const results = await Promise.all(
              project.packages.map(async (pkg) => {
                const result = await api.searchProject(project.apiChannel, pkg.path, request.query);
                return { hits: prefix(pkg.folder, result.hits), truncated: result.truncated };
              }),
            );
            if (!cancelled) {
              setState({
                hits: results.flatMap((result) => result.hits),
                truncated: results.some((result) => result.truncated),
                subject: null,
                loading: false,
                error: null,
              });
            }
          } else {
            const results = await Promise.all(
              project.packages.map(async (pkg) => {
                const usages = await api.findUsages(project.apiChannel, pkg.path, request.target);
                return { ...usages, hits: prefix(pkg.folder, usages.hits) };
              }),
            );
            const owner = results.find((usages) => usages.subject !== "");
            if (!cancelled) {
              setState({
                hits: results.flatMap((usages) => usages.hits),
                truncated: false,
                subject: owner ? { name: owner.subject, kind: owner.subjectKind } : { name: request.label, kind: "" },
                loading: false,
                error: null,
              });
            }
          }
          setActive(0);
        } catch (error) {
          if (!cancelled) {
            setState((current) => ({ ...current, loading: false, error: toCommandError(error) }));
          }
        }
      },
      request.mode === "text" ? SEARCH_DELAY : 0,
    );
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [request, project.packages, project.apiChannel, multiplePackages]);

  const groups = useMemo(() => {
    const result: { location: string; hits: { hit: SearchHit; index: number }[] }[] = [];
    state.hits.forEach((hit, index) => {
      const group = result.find((candidate) => candidate.location === hit.location);
      if (group) {
        group.hits.push({ hit, index });
      } else {
        result.push({ location: hit.location, hits: [{ hit, index }] });
      }
    });
    return result;
  }, [state.hits]);

  useEffect(() => {
    if (request.mode === "usages") {
      panelRef.current?.focus();
    }
  }, [request.mode]);

  useEffect(() => {
    listRef.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const choose = (hit: SearchHit | undefined) => {
    if (hit) {
      onSelect(hit);
    }
  };

  const status = state.loading ? (
    <LoaderCircle size={14} className={styles.spin} />
  ) : state.hits.length > 0 ? (
    `${state.hits.length}${state.truncated ? "+" : ""} ${state.hits.length === 1 ? "result" : "results"}`
  ) : null;

  return (
    <div className={styles.backdrop} onMouseDown={onClose}>
      <div
        ref={panelRef}
        tabIndex={-1}
        className={styles.panel}
        role="dialog"
        aria-label={request.mode === "usages" ? "Find usages" : "Search the project"}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          } else if (event.key === "ArrowDown") {
            event.preventDefault();
            setActive((index) => Math.min(index + 1, state.hits.length - 1));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setActive((index) => Math.max(index - 1, 0));
          } else if (event.key === "Enter") {
            event.preventDefault();
            choose(state.hits[active]);
          }
        }}
      >
        <div className={styles.searchRow}>
          <Search size={16} />
          {request.mode === "usages" ? (
            <div className={styles.subject}>
              <span>Usages of</span>
              <strong>{state.subject?.name || request.label}</strong>
              {state.subject?.kind && <span className={styles.subjectKind}>{state.subject.kind}</span>}
              <button
                type="button"
                className={styles.clear}
                title="Search text instead"
                onClick={() => onRequestChange({ mode: "text", query: "" })}
              >
                <X size={13} />
              </button>
            </div>
          ) : (
            <input
              autoFocus
              className={styles.input}
              value={query}
              placeholder="Search graphs, nodes, variables, comments and text"
              spellCheck={false}
              aria-label="Search the project"
              onChange={(event) => onRequestChange({ mode: "text", query: event.target.value })}
            />
          )}
          <span className={styles.status}>{status}</span>
        </div>

        <div ref={listRef} className={styles.results} role="listbox">
          {state.error ? (
            <div className={styles.empty}>
              <strong>{state.error.title}</strong>
              {state.error.message}
            </div>
          ) : groups.length === 0 ? (
            <div className={styles.empty}>
              {state.loading
                ? "Searching"
                : request.mode === "usages"
                  ? `Nothing in the project uses ${state.subject?.name || request.label} yet.`
                  : query.trim() === ""
                    ? "Type to search every graph of the project. Results open the graph and jump to the node."
                    : `Nothing matches "${query.trim()}".`}
            </div>
          ) : (
            groups.map((group) => (
              <div key={group.location} className={styles.group}>
                <div className={styles.location}>{group.location}</div>
                {group.hits.map(({ hit, index }) => (
                  <button
                    key={`${hit.graphId}:${hit.node ?? hit.kind}:${hit.title}:${index}`}
                    type="button"
                    role="option"
                    data-index={index}
                    aria-selected={index === active}
                    className={styles.hit}
                    data-kind={hit.kind}
                    onMouseMove={() => setActive(index)}
                    onClick={() => choose(hit)}
                  >
                    <span className={styles.icon}>{KIND_ICONS[hit.kind]}</span>
                    <span className={styles.title}>{highlighted(hit.title, query)}</span>
                    <span className={styles.detail}>{highlighted(hit.detail, query)}</span>
                    {index === active && <CornerDownLeft size={13} className={styles.enter} />}
                  </button>
                ))}
              </div>
            ))
          )}
        </div>

        <div className={styles.footer}>
          <span>
            <kbd>↑</kbd> <kbd>↓</kbd> to move
          </span>
          <span>
            <kbd>Enter</kbd> to open
          </span>
          <span>
            <kbd>Esc</kbd> to close
          </span>
        </div>
      </div>
    </div>
  );
}
