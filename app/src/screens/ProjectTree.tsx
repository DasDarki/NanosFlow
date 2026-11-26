import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  Blocks,
  Boxes,
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  FolderPlus,
  FolderSearch,
  Library,
  Megaphone,
  Plug,
  ListOrdered,
  Monitor,
  Package,
  Pencil,
  Plus,
  Server,
  Share2,
  SquareFunction,
  Trash2,
  Workflow,
  Radio,
  Search,
  Shapes,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { ContextMenu, type MenuItem } from "../components/ContextMenu";
import type { EntryLocation, FolderSummary, GraphKind, GraphSummary, PackageSummary, ProjectView, ScriptContext } from "../types";
import styles from "./ProjectTree.module.css";

export type TreeEntry =
  | { type: "graph"; graph: GraphSummary; pkg: PackageSummary }
  | { type: "folder"; folder: FolderSummary; pkg: PackageSummary };

export type CreateRequest = GraphKind | "folder";

interface ProjectTreeProps {
  project: ProjectView;
  selectedGraphId: string | null;
  nodeCounts: Record<string, number>;
  onSelectGraph: (graph: GraphSummary) => void;
  onCreate: (location: EntryLocation, request: CreateRequest) => void;
  onCreateMethod: (classGraph: GraphSummary) => void;
  onCreateDispatcher: (classGraph: GraphSummary) => void;
  onRename: (entry: TreeEntry) => void;
  onDelete: (entry: TreeEntry) => void;
  onMove: (entry: TreeEntry, target: EntryLocation) => void;
  onFindUsages: (graph: GraphSummary) => void;
}

const CONTEXTS: { context: ScriptContext; label: string; icon: ReactNode }[] = [
  { context: "server", label: "Server", icon: <Server size={13} /> },
  { context: "client", label: "Client", icon: <Monitor size={13} /> },
  { context: "shared", label: "Shared", icon: <Share2 size={13} /> },
];

const DRAG_THRESHOLD = 5;

function sameFolders(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((segment, index) => segment === right[index]);
}

function locationKey(location: EntryLocation): string {
  return [location.packagePath, location.context, ...location.folders].join("\u0000");
}

function entryLocation(entry: TreeEntry): EntryLocation {
  const item = entry.type === "graph" ? entry.graph : entry.folder;
  return { packagePath: entry.pkg.path, context: item.context, folders: item.folders };
}

function canDrop(entry: TreeEntry, target: EntryLocation): boolean {
  if (entry.pkg.path !== target.packagePath) {
    return false;
  }
  const current = entryLocation(entry);
  if (current.context === target.context && sameFolders(current.folders, target.folders)) {
    return false;
  }
  if (entry.type === "folder" && entry.folder.context === target.context) {
    const own = [...entry.folder.folders, entry.folder.name];
    const isInside = own.every((segment, index) => target.folders[index] === segment);
    if (isInside && target.folders.length >= own.length) {
      return false;
    }
  }
  return true;
}

interface DragState {
  entry: TreeEntry;
  start: { x: number; y: number };
  active: boolean;
  pointer: { x: number; y: number };
  targetKey: string | null;
}

export function ProjectTree({
  project,
  selectedGraphId,
  nodeCounts,
  onSelectGraph,
  onCreate,
  onCreateMethod,
  onCreateDispatcher,
  onRename,
  onDelete,
  onMove,
  onFindUsages,
}: ProjectTreeProps) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [menu, setMenu] = useState<{ position: { x: number; y: number }; items: MenuItem[] } | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const targetsRef = useRef(new Map<string, EntryLocation>());
  const suppressClickRef = useRef(false);

  targetsRef.current.clear();

  const toggle = (key: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });

  const setDragState = useCallback((state: DragState | null) => {
    dragRef.current = state;
    setDrag(state);
  }, []);

  useEffect(() => {
    function onPointerMove(event: PointerEvent) {
      const current = dragRef.current;
      if (!current) {
        return;
      }
      const pointer = { x: event.clientX, y: event.clientY };
      const moved = Math.hypot(pointer.x - current.start.x, pointer.y - current.start.y) > DRAG_THRESHOLD;
      if (!current.active && !moved) {
        return;
      }
      const element = document.elementFromPoint(pointer.x, pointer.y)?.closest<HTMLElement>("[data-drop]");
      const key = element?.dataset.drop ?? null;
      const target = key ? targetsRef.current.get(key) : undefined;
      setDragState({ ...current, active: true, pointer, targetKey: target && canDrop(current.entry, target) ? key : null });
    }
    function onPointerUp() {
      const current = dragRef.current;
      if (!current) {
        return;
      }
      if (current.active) {
        suppressClickRef.current = true;
        window.setTimeout(() => {
          suppressClickRef.current = false;
        }, 0);
        const target = current.targetKey ? targetsRef.current.get(current.targetKey) : undefined;
        if (target) {
          onMove(current.entry, target);
        }
      }
      setDragState(null);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    };
  }, [onMove, setDragState]);

  const startDrag = (entry: TreeEntry) => (event: React.PointerEvent) => {
    if (event.button === 0) {
      setDragState({ entry, start: { x: event.clientX, y: event.clientY }, active: false, pointer: { x: 0, y: 0 }, targetKey: null });
    }
  };

  const createItems = (location: EntryLocation): MenuItem[] => [
    { label: "New event graph", icon: <Workflow size={14} />, onSelect: () => onCreate(location, "event-graph") },
    { label: "New function", icon: <SquareFunction size={14} />, onSelect: () => onCreate(location, "function") },
    { label: "New macro", icon: <Blocks size={14} />, onSelect: () => onCreate(location, "macro") },
    { label: "New class", icon: <Boxes size={14} />, onSelect: () => onCreate(location, "class") },
    { label: "New network event", icon: <Radio size={14} />, onSelect: () => onCreate(location, "event") },
    { label: "New struct", icon: <Shapes size={14} />, onSelect: () => onCreate(location, "struct") },
    { label: "New enum", icon: <ListOrdered size={14} />, onSelect: () => onCreate(location, "enum") },
    { label: "New interface", icon: <Plug size={14} />, onSelect: () => onCreate(location, "interface") },
    { label: "New folder", icon: <FolderPlus size={14} />, onSelect: () => onCreate(location, "folder") },
  ];

  const entryItems = (entry: TreeEntry, path: string): MenuItem[] => [
    { label: "Rename", icon: <Pencil size={14} />, separatorBefore: entry.type === "folder", onSelect: () => onRename(entry) },
    { label: "Show in file manager", icon: <FolderSearch size={14} />, onSelect: () => void revealItemInDir(path) },
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, separatorBefore: true, onSelect: () => onDelete(entry) },
  ];

  const openMenu = (event: React.MouseEvent, items: MenuItem[]) => {
    event.preventDefault();
    event.stopPropagation();
    setMenu({ position: { x: event.clientX, y: event.clientY }, items });
  };

  const addButton = (location: EntryLocation, label: string) => (
    <button
      type="button"
      className={styles.addButton}
      title={`Add to ${label}`}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation();
        const bounds = event.currentTarget.getBoundingClientRect();
        setMenu({ position: { x: bounds.left, y: bounds.bottom + 4 }, items: createItems(location) });
      }}
    >
      <Plus size={14} />
    </button>
  );

  function renderGraph(pkg: PackageSummary, graph: GraphSummary, depth: number): ReactNode {
    const entry: TreeEntry = { type: "graph", graph, pkg };
    const isMethod = Boolean(graph.owner);
    const methods = graph.kind === "class" ? pkg.graphs.filter((candidate) => candidate.owner === graph.id) : [];
    const isCollapsed = collapsed.has(graph.path);
    const items: MenuItem[] = graph.kind === "class"
      ? [
          { label: "New method", icon: <SquareFunction size={14} />, onSelect: () => onCreateMethod(graph) },
          { label: "New event dispatcher", icon: <Megaphone size={14} />, onSelect: () => onCreateDispatcher(graph) },
          ...entryItems(entry, graph.path),
        ]
      : entryItems(entry, graph.path);
    if (graph.kind !== "event-graph") {
      items.unshift({ label: "Find usages", icon: <Search size={14} />, onSelect: () => onFindUsages(graph) });
    }
    return (
      <div key={graph.id}>
        <div
          className={styles.row}
          data-graph
          data-dragging={drag?.active && drag.entry.type === "graph" && drag.entry.graph.id === graph.id}
          style={{ "--depth": depth } as CSSProperties}
          role="treeitem"
          aria-selected={selectedGraphId === graph.id}
          aria-expanded={graph.kind === "class" ? !isCollapsed : undefined}
          tabIndex={0}
          title={graph.kind === "class" && graph.parent ? `${graph.name} inherits from ${graph.parent}` : graph.path}
          onPointerDown={isMethod ? undefined : startDrag(entry)}
          onClick={() => !suppressClickRef.current && onSelectGraph(graph)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              onSelectGraph(graph);
            } else if (event.key === "F2") {
              onRename(entry);
            } else if (event.key === "Delete") {
              onDelete(entry);
            }
          }}
          onContextMenu={(event) => openMenu(event, items)}
        >
          <span
            className={styles.chevron}
            onClick={(event) => {
              if (methods.length > 0) {
                event.stopPropagation();
                toggle(graph.path);
              }
            }}
          >
            {methods.length > 0 && (isCollapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />)}
          </span>
          <span className={styles.graphIcon} data-kind={graph.kind}>
            {graph.kind === "class" ? (
              <Boxes size={14} />
            ) : graph.kind === "function" ? (
              <SquareFunction size={14} />
            ) : graph.kind === "event" ? (
              <Radio size={14} />
            ) : graph.kind === "struct" ? (
              <Shapes size={14} />
            ) : graph.kind === "enum" ? (
              <ListOrdered size={14} />
            ) : graph.kind === "dispatcher" ? (
              <Megaphone size={14} />
            ) : graph.kind === "interface" ? (
              <Plug size={14} />
            ) : graph.kind === "macro" ? (
              <Blocks size={14} />
            ) : (
              <Workflow size={14} />
            )}
          </span>
          <span className={styles.label}>{graph.name}</span>
          {graph.kind === "class" && graph.parent && <span className={styles.parent}>{graph.parent}</span>}
          {graph.exported && (
            <span className={styles.exported} title="Exported. Packages that require this package can call it.">
              <Library size={12} />
            </span>
          )}
          {!["event", "struct", "enum", "dispatcher", "interface"].includes(graph.kind) && <span className={styles.count}>{nodeCounts[graph.id] ?? graph.nodeCount}</span>}
        </div>
        {!isCollapsed && methods.map((method) => renderGraph(pkg, method, depth + 1))}
      </div>
    );
  }

  function renderLevel(pkg: PackageSummary, context: ScriptContext, folders: string[], depth: number): ReactNode {
    const childFolders = pkg.folders.filter((folder) => folder.context === context && sameFolders(folder.folders, folders));
    const childGraphs = pkg.graphs.filter(
      (graph) => !graph.owner && graph.context === context && sameFolders(graph.folders, folders),
    );
    const indent = { "--depth": depth } as CSSProperties;

    return (
      <>
        {childFolders.map((folder) => {
          const location: EntryLocation = { packagePath: pkg.path, context, folders: [...folders, folder.name] };
          const key = locationKey(location);
          targetsRef.current.set(key, location);
          const entry: TreeEntry = { type: "folder", folder, pkg };
          const isCollapsed = collapsed.has(folder.path);
          const childPath = [...folders, folder.name];
          const isEmpty =
            !pkg.folders.some((candidate) => candidate.context === context && sameFolders(candidate.folders, childPath)) &&
            !pkg.graphs.some((candidate) => candidate.context === context && sameFolders(candidate.folders, childPath));
          return (
            <div key={folder.path}>
              <div
                className={styles.row}
                data-drop={key}
                data-drop-active={drag?.targetKey === key}
                data-dragging={drag?.active && drag.entry.type === "folder" && drag.entry.folder.path === folder.path}
                style={indent}
                role="treeitem"
                aria-expanded={!isCollapsed}
                tabIndex={0}
                onPointerDown={startDrag(entry)}
                onClick={() => !suppressClickRef.current && toggle(folder.path)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    toggle(folder.path);
                  } else if (event.key === "F2") {
                    onRename(entry);
                  } else if (event.key === "Delete") {
                    onDelete(entry);
                  }
                }}
                onContextMenu={(event) => openMenu(event, [...createItems(location), ...entryItems(entry, folder.path)])}
              >
                <span className={styles.chevron}>
                  {!isEmpty && (isCollapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />)}
                </span>
                <span className={styles.folderIcon}>{isCollapsed || isEmpty ? <Folder size={14} /> : <FolderOpen size={14} />}</span>
                <span className={styles.label}>{folder.name}</span>
                {addButton(location, folder.name)}
              </div>
              {!isCollapsed && renderLevel(pkg, context, [...folders, folder.name], depth + 1)}
            </div>
          );
        })}
        {childGraphs.map((graph) => renderGraph(pkg, graph, depth))}
      </>
    );
  }

  return (
    <div className={styles.tree} role="tree" aria-label="Project">
      {project.packages.map((pkg) => (
        <div key={pkg.id} className={styles.package}>
          <div className={styles.packageHeader}>
            <Package size={15} />
            <span className={styles.label}>{pkg.folder}</span>
          </div>
          {CONTEXTS.map(({ context, label, icon }) => {
            const location: EntryLocation = { packagePath: pkg.path, context, folders: [] };
            const key = locationKey(location);
            targetsRef.current.set(key, location);
            const sectionKey = `${pkg.path}:${context}`;
            const isCollapsed = collapsed.has(sectionKey);
            const isEmpty = !pkg.graphs.some((graph) => graph.context === context) && !pkg.folders.some((folder) => folder.context === context);
            return (
              <div key={context} className={styles.section}>
                <div
                  className={styles.sectionHeader}
                  data-context={context}
                  data-drop={key}
                  data-drop-active={drag?.targetKey === key}
                  onClick={() => toggle(sectionKey)}
                  onContextMenu={(event) => openMenu(event, createItems(location))}
                >
                  <span className={styles.chevron}>{isCollapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}</span>
                  <span className={styles.sectionIcon}>{icon}</span>
                  {label}
                  {addButton(location, label)}
                </div>
                {!isCollapsed &&
                  (isEmpty ? (
                    <p className={styles.empty}>Empty. Right-click or use + to add a graph.</p>
                  ) : (
                    renderLevel(pkg, context, [], 1)
                  ))}
              </div>
            );
          })}
        </div>
      ))}

      {drag?.active && (
        <div className={styles.dragGhost} style={{ left: drag.pointer.x + 12, top: drag.pointer.y + 8 }} data-valid={drag.targetKey !== null}>
          {drag.entry.type === "graph" ? drag.entry.graph.name : drag.entry.folder.name}
          <span>{drag.targetKey ? "Drop to move here" : "Drop on a folder or section"}</span>
        </div>
      )}

      {menu && <ContextMenu position={menu.position} items={menu.items} onClose={() => setMenu(null)} />}
    </div>
  );
}
