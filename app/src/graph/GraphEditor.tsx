import {
  Background,
  BackgroundVariant,
  ConnectionMode,
  MiniMap,
  Panel,
  ReactFlow,
  ReactFlowProvider,
  useConnection,
  useReactFlow,
  type Connection,
  type EdgeChangeset,
  type FinalConnectionState,
  type NodeChangeset,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { RemoteCursors, type RemotePeer } from "./RemoteCursors";
import {
  CircleAlert,
  CloudOff,
  FileCode,
  GitCompareArrows,
  GitMerge,
  RotateCcw,
  LoaderCircle,
  Maximize,
  MousePointerClick,
  PanelRight,
  Plus,
  Redo2,
  Undo2,
  Check,
  Copy,
  MessageSquare,
  CircleDot,
  Eye,
  EyeOff,
  Search,
  SquareFunction,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { ContextMenu, type MenuItem } from "../components/ContextMenu";
import { useToasts } from "../components/Toasts";
import type {
  ApiChannel,
  BuildProblem,
  CommandError,
  RuntimeMark,
  FieldIssue,
  GraphSummary,
  LuaPreview as LuaPreviewData,
  ProjectView,
} from "../types";
import { formatType, type NodeCatalog } from "./catalog";
import { CommentNode } from "./CommentNode";
import {
  COMMENT,
  COMMENT_SIZE,
  documentsToFlow,
  EXEC_REROUTE,
  flowToDocuments,
  flowTypeOf,
  makeEdge,
  REROUTE,
  serializeDocument,
  shapeNode,
  type FlowEdge,
  type FlowNode,
} from "./convert";
import { EditorContext, pinKey, type EditorApi } from "./EditorContext";
import { initialProperties } from "./dynamic";
import { FlowNodeView } from "./FlowNode";
import { TypeResolver } from "./generics";
import { ClassSettings } from "./ClassSettings";
import { FunctionSignatureEditor } from "./FunctionSignatureEditor";
import { TimelineEditor } from "./TimelineEditor";
import { MergePanel } from "./MergePanel";
import { computeChanges } from "./changes";
import type { CommittedGraph, ConflictChoice, FileConflict } from "./model";
import type {
  Diagnostic,
  FunctionSignature,
  LiteralValue,
  NodeDefinition,
  NodeDocument,
  NodeLoadError,
  PinDefinition,
  PinType,
  ScopeData,
  VariableDocument,
} from "./model";
import { VariablesPanel, type PlaceMode } from "./VariablesPanel";
import { LuaPreview } from "./LuaPreview";
import { NodeDocs } from "./NodeDocs";
import { NodePalette, type PaletteFilter } from "./NodePalette";
import { definitionColor, pinColor } from "./pinStyle";
import { RerouteNode } from "./RerouteNode";
import { ProblemsPanel } from "./ProblemsPanel";
import { generateUlid } from "./ulid";
import { ConnectionLine, Wire } from "./Wire";
import styles from "./GraphEditor.module.css";

const NODE_TYPES = { flow: FlowNodeView, comment: CommentNode, reroute: RerouteNode };
const CLIPBOARD_FORMAT = 1;
const COMMENT_PADDING = 32;
const EDGE_TYPES = { wire: Wire };
const GRID = 16;
const HISTORY_LIMIT = 200;
const SAVE_DELAY = 350;
const ANALYZE_DELAY = 200;
const LITERAL_COALESCE_MILLISECONDS = 1000;
const SIGNATURE_DELAY = 450;
const FUNCTION_ENTRY = "function/entry";
const MACRO_INPUTS = "macro/inputs";
const PROTECTED_NODES = new Set([FUNCTION_ENTRY, MACRO_INPUTS, "macro/outputs"]);

interface Snapshot {
  nodes: FlowNode[];
  edges: FlowEdge[];
}

interface PaletteSource {
  nodeId: string;
  handle: string;
  fromDirection: "input" | "output";
  pinType: PinType;
}

interface PaletteState {
  screen: { x: number; y: number };
  filter?: PaletteFilter;
  source?: PaletteSource;
}

interface ConnectionCheck {
  valid: boolean;
  message: string;
}

type SaveState = "saved" | "saving" | "failed";

interface ClipboardPayload {
  nanosflow: number;
  nodes: NodeDocument[];
  titles: Record<string, string>;
}

let clipboard: ClipboardPayload | null = null;

function isClipboardPayload(value: unknown): value is ClipboardPayload {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Partial<ClipboardPayload>;
  return candidate.nanosflow === CLIPBOARD_FORMAT && Array.isArray(candidate.nodes);
}

function remapDocuments(documents: NodeDocument[], idMap: Map<string, string>, offset: { x: number; y: number }): NodeDocument[] {
  return documents.map((document) => {
    const inputs = Object.fromEntries(
      Object.entries(document.inputs ?? {}).filter(([, value]) => !("node" in value) || idMap.has(value.node)).map(([pin, value]) => [
        pin,
        "node" in value ? { node: idMap.get(value.node)!, pin: value.pin } : value,
      ]),
    );
    const exec = Object.fromEntries(
      Object.entries(document.exec ?? {})
        .filter(([, link]) => idMap.has(link.node))
        .map(([pin, link]) => [pin, { node: idMap.get(link.node)!, pin: link.pin }]),
    );
    return {
      ...document,
      id: idMap.get(document.id)!,
      position: [document.position[0] + offset.x, document.position[1] + offset.y],
      inputs,
      exec,
    };
  });
}

function snap(value: number): number {
  return Math.round(value / GRID) * GRID;
}

function freshNode(nodeType: string, position: { x: number; y: number }, initial: Record<string, unknown> = {}): FlowNode {
  const properties: Record<string, unknown> = nodeType === COMMENT ? { text: "Comment", ...COMMENT_SIZE } : initial;
  return shapeNode({
    id: generateUlid(),
    type: flowTypeOf(nodeType),
    position,
    selected: true,
    data: { nodeType, literals: {}, properties, staleInputs: {}, staleExec: {} },
  });
}

export interface GraphEditorProps {
  graph: GraphSummary;
  packageFolder: string;
  packagePath: string;
  apiChannel: ApiChannel;
  catalog: NodeCatalog;
  onNodeCountChange: (graphId: string, count: number) => void;
  onRegisterFlush: (flush: (() => Promise<void>) | null) => void;
  projectFile: string;
  onProjectChange: (project: ProjectView) => void;
  focusRequest: FocusRequest | null;
  sidePanel: SidePanel | null;
  onSidePanelChange: (panel: SidePanel | null) => void;
  onCollapseSelection: (selection: string[]) => void;
  onFindUsages: (target: string, label: string) => void;
  runtime: Record<string, RuntimeMark>;
  scopeRevision: number;
  remoteChange: RemoteChange | null;
  peers: RemotePeer[];
  onPresence: ((presence: { x: number; y: number; selection: string[] }) => void) | null;
}

export interface RemoteChange {
  revision: number;
  paths: string[];
  undoable?: boolean;
}

const PRESENCE_INTERVAL = 50;

export function normalizePath(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "");
}

const USAGE_TARGET = /^(?:function|method|macro|library|interface\/implements|dispatcher\/(?:call|bind|unbind|unbind-all)|class\/spawn|custom-event|send-event|struct\/(?:make|break|set)|variable\/(?:get|set))\/([0-9a-z]{26})(?:\/|$)/i;

function usageTarget(nodeType: string): string | null {
  return USAGE_TARGET.exec(nodeType)?.[1] ?? null;
}

export interface FocusRequest {
  graphId: string;
  node: string;
  token: number;
}

export type SidePanel = "details" | "lua";

export interface LuaPreviewState {
  data: LuaPreviewData | null;
  error: CommandError | null;
  loading: boolean;
}

const LUA_REFRESH_DELAY = 450;
const NO_CHANGES: Record<string, never> = {};

function libraryName(folder: string): string {
  const words = folder.match(/[A-Za-z0-9]+/g) ?? [];
  const joined = words.map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join("");
  return joined === "" ? "Unnamed" : /^[0-9]/.test(joined) ? `N${joined}` : joined;
}

function withEntryProtection(nodes: FlowNode[]): FlowNode[] {
  return nodes.map((node) => (PROTECTED_NODES.has(node.data.nodeType) ? { ...node, deletable: false } : node));
}

export function GraphEditor(props: GraphEditorProps) {
  return (
    <ReactFlowProvider>
      <Editor {...props} />
    </ReactFlowProvider>
  );
}

function ConnectionHint({ hint }: { hint: ConnectionCheck | null }) {
  const connection = useConnection();
  if (!connection.inProgress) {
    return null;
  }
  const overHandle = connection.toHandle !== null;
  const message = overHandle && hint ? hint.message : "Release on empty space to add a node that fits this pin";
  const valid = overHandle && hint ? hint.valid : true;
  return (
    <Panel position="top-center">
      <div className={styles.connectionHint} data-valid={valid}>
        {!valid && <CircleAlert size={14} />}
        {message}
      </div>
    </Panel>
  );
}

function Editor({
  graph,
  packageFolder,
  packagePath,
  scopeRevision,
  apiChannel,
  catalog: baseCatalog,
  onNodeCountChange,
  onRegisterFlush,
  projectFile,
  onProjectChange,
  focusRequest,
  sidePanel,
  onSidePanelChange,
  onCollapseSelection,
  onFindUsages,
  runtime,
  remoteChange,
  peers,
  onPresence,
}: GraphEditorProps) {
  const { notify, reportError } = useToasts();
  const flow = useReactFlow<FlowNode, FlowEdge>();
  const [nodes, setNodes] = useState<FlowNode[]>([]);
  const [edges, setEdges] = useState<FlowEdge[]>([]);
  const [status, setStatus] = useState<"loading" | "ready" | "failed">("loading");
  const [failure, setFailure] = useState<CommandError | null>(null);
  const [loadErrors, setLoadErrors] = useState<NodeLoadError[]>([]);
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  const [saveState, setSaveState] = useState<SaveState>("saved");
  const [palette, setPalette] = useState<PaletteState | null>(null);
  const [connectionHint, setConnectionHint] = useState<ConnectionCheck | null>(null);
  const [luaRevision, setLuaRevision] = useState(0);
  const [luaPreview, setLuaPreview] = useState<LuaPreviewState>({ data: null, error: null, loading: true });
  const [historySize, setHistorySize] = useState({ past: 0, future: 0 });
  const [catalog, setCatalog] = useState<NodeCatalog>(baseCatalog);
  const [functionSignature, setFunctionSignature] = useState<FunctionSignature | null>(null);
  const [signatureIssues, setSignatureIssues] = useState<FieldIssue[]>([]);
  const signatureDirtyRef = useRef(false);
  const [classParent, setClassParent] = useState<string | null>(null);
  const [classInterfaces, setClassInterfaces] = useState<string[]>([]);
  const [classExported, setClassExported] = useState(false);
  const [timelineNode, setTimelineNode] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [conflicts, setConflicts] = useState<FileConflict[] | null>(null);
  const [mergeBusy, setMergeBusy] = useState(false);
  const [committed, setCommitted] = useState<CommittedGraph | null>(null);
  const [changesLoading, setChangesLoading] = useState(false);
  const [projectInterfaces, setProjectInterfaces] = useState<string[]>([]);
  const [classDescription, setClassDescription] = useState("");
  const [classIssue, setClassIssue] = useState<FieldIssue | null>(null);
  const classDescriptionDirtyRef = useRef(false);
  const [variables, setVariables] = useState<VariableDocument[]>([]);
  const [variableIssues, setVariableIssues] = useState<FieldIssue[]>([]);
  const variablesRef = useRef<VariableDocument[]>([]);
  const variablesDirtyRef = useRef(false);
  const savedVariableIdsRef = useRef<Set<string>>(new Set());
  const [placing, setPlacing] = useState<{ label: string; x: number; y: number } | null>(null);

  const nodesRef = useRef<FlowNode[]>([]);
  const edgesRef = useRef<FlowEdge[]>([]);
  const savedRef = useRef<Map<string, string>>(new Map());
  const historyRef = useRef<{ past: Snapshot[]; future: Snapshot[] }>({ past: [], future: [] });
  const pushedThisTickRef = useRef(false);
  const draggingRef = useRef(false);
  const lastLiteralRef = useRef({ key: "", time: 0 });
  const pointerRef = useRef<{ x: number; y: number } | null>(null);
  const rightPressRef = useRef<{ x: number; y: number } | null>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const saveChainRef = useRef<Promise<void>>(Promise.resolve());

  const update = useCallback((nextNodes: FlowNode[], nextEdges: FlowEdge[]) => {
    nodesRef.current = nextNodes;
    edgesRef.current = nextEdges;
    setNodes(nextNodes);
    setEdges(nextEdges);
  }, []);

  const syncHistorySize = useCallback(() => {
    setHistorySize({ past: historyRef.current.past.length, future: historyRef.current.future.length });
  }, []);

  const pushHistory = useCallback(() => {
    if (pushedThisTickRef.current) {
      return;
    }
    pushedThisTickRef.current = true;
    queueMicrotask(() => {
      pushedThisTickRef.current = false;
    });
    const history = historyRef.current;
    history.past.push({ nodes: nodesRef.current, edges: edgesRef.current });
    if (history.past.length > HISTORY_LIMIT) {
      history.past.shift();
    }
    history.future = [];
    lastLiteralRef.current = { key: "", time: 0 };
    syncHistorySize();
  }, [syncHistorySize]);

  const undo = useCallback(() => {
    const history = historyRef.current;
    const snapshot = history.past.pop();
    if (!snapshot) {
      return;
    }
    history.future.push({ nodes: nodesRef.current, edges: edgesRef.current });
    update(snapshot.nodes, snapshot.edges);
    syncHistorySize();
  }, [update, syncHistorySize]);

  const redo = useCallback(() => {
    const history = historyRef.current;
    const snapshot = history.future.pop();
    if (!snapshot) {
      return;
    }
    history.past.push({ nodes: nodesRef.current, edges: edgesRef.current });
    update(snapshot.nodes, snapshot.edges);
    syncHistorySize();
  }, [update, syncHistorySize]);

  useEffect(() => {
    let cancelled = false;
    setStatus("loading");
    Promise.all([api.loadGraph(graph.path), api.graphScope(apiChannel, packagePath, graph.path)])
      .then(([document, scope]) => {
        if (cancelled) {
          return;
        }
        const scoped = baseCatalog.extend(scope.definitions, scope.classes, scope.structs, scope.enums);
        setCatalog(scoped);
        setFunctionSignature(document.manifest.kind === "function" || document.manifest.kind === "macro" ? document.manifest : null);
        setClassParent(document.manifest.kind === "class" ? (document.manifest.parent ?? null) : null);
        setClassInterfaces(document.manifest.interfaces ?? []);
        setClassExported(document.manifest.exported ?? false);
        setProjectInterfaces(scope.interfaces);
        setClassDescription(document.manifest.description ?? "");
        classDescriptionDirtyRef.current = false;
        variablesRef.current = document.variables;
        variablesDirtyRef.current = false;
        savedVariableIdsRef.current = new Set(document.variables.map((variable) => variable.id));
        setVariables(document.variables);
        const loaded = documentsToFlow(document.nodes, scoped);
        loaded.nodes = withEntryProtection(loaded.nodes);
        savedRef.current = new Map(
          flowToDocuments(loaded.nodes, loaded.edges).map((node) => [node.id, serializeDocument(node)]),
        );
        historyRef.current = { past: [], future: [] };
        syncHistorySize();
        update(loaded.nodes, loaded.edges);
        setLoadErrors(document.loadErrors);
        setStatus("ready");
        if (document.loadErrors.some((error) => !error.hasConflictMarkers)) {
          notify({
            kind: "warning",
            title: "Some nodes could not be loaded",
            message: `${document.loadErrors.filter((error) => !error.hasConflictMarkers).length} files were skipped. See Problems for details.`,
          });
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setFailure(toCommandError(error));
          setStatus("failed");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [graph.path, packagePath, apiChannel, baseCatalog, update, notify, syncHistorySize, reloadToken]);

  const documents = useMemo(() => flowToDocuments(nodes, edges), [nodes, edges]);
  const serialized = useMemo(() => new Map(documents.map((node) => [node.id, serializeDocument(node)])), [documents]);
  const signature = useMemo(() => [...serialized.values()].join("\n"), [serialized]);
  const latestRef = useRef({ documents, serialized });
  latestRef.current = { documents, serialized };

  const save = useCallback((): Promise<void> => {
    const { documents: currentDocuments, serialized: current } = latestRef.current;
    const saved = savedRef.current;
    const upserts = currentDocuments.filter((node) => saved.get(node.id) !== current.get(node.id));
    const deletions = [...saved.keys()].filter((id) => !current.has(id));
    if (upserts.length === 0 && deletions.length === 0) {
      return saveChainRef.current;
    }
    savedRef.current = current;
    setSaveState("saving");
    saveChainRef.current = saveChainRef.current
      .then(() => api.saveGraphChanges(graph.path, upserts, deletions))
      .then(
        () => {
          setSaveState("saved");
          setLuaRevision((value) => value + 1);
        },
        (error) => {
          const retry = new Map(savedRef.current);
          for (const node of upserts) {
            retry.delete(node.id);
          }
          for (const id of deletions) {
            retry.set(id, "");
          }
          savedRef.current = retry;
          setSaveState("failed");
          reportError(toCommandError(error));
        },
      );
    return saveChainRef.current;
  }, [graph.path, reportError]);

  useEffect(() => {
    if (status !== "ready") {
      return;
    }
    const timer = window.setTimeout(() => void save(), SAVE_DELAY);
    return () => window.clearTimeout(timer);
  }, [signature, status, save]);

  useEffect(() => {
    if (status !== "ready") {
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      api
        .analyzeGraph(apiChannel, packagePath, graph.path, graph.context, graph.kind, latestRef.current.documents)
        .then((result) => {
          if (!cancelled) {
            setDiagnostics(result);
          }
        })
        .catch(() => undefined);
    }, ANALYZE_DELAY);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [signature, status, apiChannel, packagePath, graph.path, graph.context, graph.kind, catalog]);

  useEffect(() => {
    onRegisterFlush(save);
    return () => onRegisterFlush(null);
  }, [save, onRegisterFlush]);

  const applyScope = useCallback(
    (scope: ScopeData) => {
      const scoped = baseCatalog.extend(scope.definitions, scope.classes, scope.structs, scope.enums);
      setProjectInterfaces(scope.interfaces);
      const selected = new Set(nodesRef.current.filter((node) => node.selected).map((node) => node.id));
      const documents = flowToDocuments(nodesRef.current, edgesRef.current);
      const projected = documentsToFlow(documents, scoped);
      historyRef.current = { past: [], future: [] };
      syncHistorySize();
      setCatalog(scoped);
      setLuaRevision((value) => value + 1);
      update(
        withEntryProtection(projected.nodes).map((node) => (selected.has(node.id) ? { ...node, selected: true } : node)),
        projected.edges,
      );
    },
    [baseCatalog, syncHistorySize, update],
  );

  const appliedScopeRevisionRef = useRef(scopeRevision);
  useEffect(() => {
    if (appliedScopeRevisionRef.current === scopeRevision) {
      return;
    }
    appliedScopeRevisionRef.current = scopeRevision;
    let cancelled = false;
    api
      .graphScope(apiChannel, packagePath, graph.path)
      .then((scope) => {
        if (!cancelled) {
          applyScope(scope);
        }
      })
      .catch((error) => reportError(toCommandError(error)));
    return () => {
      cancelled = true;
    };
  }, [scopeRevision, apiChannel, packagePath, graph.path, applyScope, reportError]);

  useEffect(() => {
    if (!functionSignature || !signatureDirtyRef.current) {
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      api
        .updateFunctionSignature(apiChannel, packagePath, graph.path, functionSignature)
        .then((scope) => {
          signatureDirtyRef.current = false;
          if (!cancelled) {
            setSignatureIssues([]);
            applyScope(scope);
          }
        })
        .catch((error) => {
          const commandError = toCommandError(error);
          if (commandError.issues.length > 0) {
            setSignatureIssues(commandError.issues);
          } else {
            reportError(commandError);
          }
        });
    }, SIGNATURE_DELAY);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [functionSignature, apiChannel, packagePath, graph.path, applyScope, reportError]);

  const saveVariables = useCallback(async (): Promise<boolean> => {
    if (!variablesDirtyRef.current) {
      return true;
    }
    const current = variablesRef.current;
    const deletions = [...savedVariableIdsRef.current].filter((id) => !current.some((variable) => variable.id === id));
    try {
      const scope = await api.saveVariables(apiChannel, packagePath, graph.path, current, deletions);
      if (variablesRef.current === current) {
        variablesDirtyRef.current = false;
      }
      savedVariableIdsRef.current = new Set(current.map((variable) => variable.id));
      setVariableIssues([]);
      applyScope(scope);
      return true;
    } catch (error) {
      const commandError = toCommandError(error);
      if (commandError.issues.length > 0) {
        setVariableIssues(commandError.issues);
      } else {
        reportError(commandError);
      }
      return false;
    }
  }, [apiChannel, packagePath, graph.path, applyScope, reportError]);

  useEffect(() => {
    if (!variablesDirtyRef.current) {
      return;
    }
    const timer = window.setTimeout(() => void saveVariables(), SIGNATURE_DELAY);
    return () => window.clearTimeout(timer);
  }, [variables, saveVariables]);

  const saveClass = useCallback(
    async (parent: string, description: string, interfaces: string[], exported: boolean, isParentChange: boolean): Promise<boolean> => {
      try {
        const result = await api.updateClass(apiChannel, projectFile, packagePath, graph.path, parent, description, interfaces, exported);
        if (isParentChange) {
          setClassIssue(null);
        }
        onProjectChange(result.project);
        applyScope(result.scope);
        return true;
      } catch (error) {
        const commandError = toCommandError(error);
        if (commandError.issues.length > 0) {
          setClassIssue(commandError.issues[0]);
        } else {
          reportError(commandError);
        }
        return false;
      }
    },
    [apiChannel, projectFile, packagePath, graph.path, onProjectChange, applyScope, reportError],
  );

  const changeClassParent = useCallback(
    (parent: string) => {
      const previous = classParent;
      setClassParent(parent);
      void saveClass(parent, classDescription, classInterfaces, classExported, true).then((saved) => {
        if (!saved) {
          setClassParent(previous);
        }
      });
    },
    [classParent, classDescription, classInterfaces, classExported, saveClass],
  );

  const changeClassExported = useCallback(
    (exported: boolean) => {
      if (classParent === null) {
        return;
      }
      setClassExported(exported);
      void saveClass(classParent, classDescription, classInterfaces, exported, false).then((saved) => {
        if (!saved) {
          setClassExported(!exported);
        }
      });
    },
    [classParent, classDescription, classInterfaces, saveClass],
  );

  const changeClassInterfaces = useCallback(
    (interfaces: string[]) => {
      if (classParent === null) {
        return;
      }
      const previous = classInterfaces;
      setClassInterfaces(interfaces);
      void saveClass(classParent, classDescription, interfaces, classExported, true).then((saved) => {
        if (!saved) {
          setClassInterfaces(previous);
        }
      });
    },
    [classParent, classDescription, classInterfaces, classExported, saveClass],
  );

  useEffect(() => {
    if (!classDescriptionDirtyRef.current || classParent === null) {
      return;
    }
    const timer = window.setTimeout(() => {
      classDescriptionDirtyRef.current = false;
      void saveClass(classParent, classDescription, classInterfaces, classExported, false);
    }, SIGNATURE_DELAY);
    return () => window.clearTimeout(timer);
  }, [classDescription, classParent, classInterfaces, classExported, saveClass]);

  const changeVariables = useCallback((next: VariableDocument[]) => {
    variablesRef.current = next;
    variablesDirtyRef.current = true;
    setVariables(next);
  }, []);

  const changeSignature = useCallback((next: FunctionSignature) => {
    signatureDirtyRef.current = true;
    setFunctionSignature(next);
  }, []);

  useEffect(() => {
    const flush = () => void save();
    window.addEventListener("beforeunload", flush);
    return () => {
      window.removeEventListener("beforeunload", flush);
      flush();
    };
  }, [save]);

  useEffect(() => {
    if (status === "ready") {
      onNodeCountChange(graph.id, nodes.length);
    }
  }, [nodes.length, status, graph.id, onNodeCountChange]);

  const nodeTypesKey = nodes.map((node) => `${node.id}:${node.data.nodeType}`).join(",");
  const resolver = useMemo(
    () => new TypeResolver(catalog, nodesRef.current, edges),
    [catalog, edges, nodeTypesKey],
  );

  const findPin = useCallback(
    (nodeId: string, handle: string | null | undefined, direction: "input" | "output") => {
      const node = nodesRef.current.find((candidate) => candidate.id === nodeId);
      const definition = node ? catalog.definitionFor(node.data.nodeType, node.data.properties) : undefined;
      const pins = direction === "input" ? definition?.inputs : definition?.outputs;
      const declared = pins?.find((candidate) => candidate.name === handle);
      return { pin: declared && { ...declared, type: resolver.pinType(nodeId, declared) }, definition };
    },
    [catalog, resolver],
  );

  const checkConnection = useCallback(
    (connection: Connection | FlowEdge): ConnectionCheck => {
      if (connection.source === connection.target) {
        return { valid: false, message: "A node cannot be connected to itself." };
      }
      const { pin: output } = findPin(connection.source, connection.sourceHandle, "output");
      const { pin: input, definition: targetDefinition } = findPin(connection.target, connection.targetHandle, "input");
      if (!output || !input) {
        return { valid: false, message: "" };
      }
      if (output.type.kind === "exec" || input.type.kind === "exec") {
        if (output.type.kind !== input.type.kind) {
          return { valid: false, message: "Execution pins can only be connected to other execution pins." };
        }
        const replaces = edgesRef.current.some(
          (edge) => edge.source === connection.source && edge.sourceHandle === connection.sourceHandle,
        );
        return {
          valid: true,
          message: replaces ? "Replace the existing execution wire" : `Continue with ${targetDefinition?.title ?? "this node"}`,
        };
      }
      if (!catalog.isAssignable(output.type.type, input.type.type)) {
        return {
          valid: false,
          message: `${formatType(output.type.type)} is not compatible with ${formatType(input.type.type)}.`,
        };
      }
      const replaces = edgesRef.current.some(
        (edge) => edge.target === connection.target && edge.targetHandle === connection.targetHandle,
      );
      return {
        valid: true,
        message: replaces
          ? `Replace the existing connection of ${input.label || input.name}`
          : `Connect ${formatType(output.type.type)} to ${input.label || input.name}`,
      };
    },
    [catalog, findPin],
  );

  const isValidConnection = useCallback(
    (connection: Connection | FlowEdge) => {
      const result = checkConnection(connection);
      setConnectionHint(result.message ? result : null);
      return result.valid;
    },
    [checkConnection],
  );

  const connect = useCallback(
    (edgesBefore: FlowEdge[], source: string, sourceHandle: string, target: string, targetHandle: string, exec: boolean) => {
      const remaining = edgesBefore.filter((edge) =>
        exec
          ? !(edge.source === source && edge.sourceHandle === sourceHandle)
          : !(edge.target === target && edge.targetHandle === targetHandle),
      );
      return [...remaining, makeEdge(source, sourceHandle, target, targetHandle, exec)];
    },
    [],
  );

  const onConnect = useCallback(
    (connection: Connection) => {
      const { pin: output } = findPin(connection.source, connection.sourceHandle, "output");
      if (!output || !connection.sourceHandle || !connection.targetHandle) {
        return;
      }
      pushHistory();
      update(
        nodesRef.current,
        connect(
          edgesRef.current,
          connection.source,
          connection.sourceHandle,
          connection.target,
          connection.targetHandle,
          output.type.kind === "exec",
        ),
      );
    },
    [connect, findPin, pushHistory, update],
  );

  const openPalette = useCallback((screen: { x: number; y: number }, filter?: PaletteFilter, source?: PaletteSource) => {
    setPalette({ screen, filter, source });
  }, []);

  const onConnectEnd = useCallback(
    (event: MouseEvent | TouchEvent, state: FinalConnectionState) => {
      setConnectionHint(null);
      if (state.isValid || state.toHandle || !state.fromHandle || !state.fromNode || !state.fromHandle.id) {
        return;
      }
      const point = "changedTouches" in event ? event.changedTouches[0] : event;
      const fromDirection = state.fromHandle.type === "source" ? "output" : "input";
      const { pin, definition } = findPin(state.fromNode.id, state.fromHandle.id, fromDirection);
      if (!pin) {
        return;
      }
      openPalette(
        { x: point.clientX, y: point.clientY },
        {
          pinType: pin.type,
          direction: fromDirection === "output" ? "input" : "output",
          pinLabel: `${pin.label || (pin.type.kind === "exec" ? "execution pin" : pin.name)} of ${definition?.title ?? "node"}`,
        },
        { nodeId: state.fromNode.id, handle: state.fromHandle.id, fromDirection, pinType: pin.type },
      );
    },
    [findPin, openPalette],
  );

  const addNode = useCallback(
    (definition: NodeDefinition, request: PaletteState) => {
      const anchor = flow.screenToFlowPosition(request.screen);
      const offsetX = request.source?.fromDirection === "input" ? -260 : 0;
      const node = freshNode(definition.id, { x: snap(anchor.x + offsetX), y: snap(anchor.y - 20) }, initialProperties(definition));
      let nextEdges = edgesRef.current;
      if (request.source) {
        const { nodeId, handle, fromDirection, pinType } = request.source;
        const pin = catalog.firstCompatiblePin(definition, pinType, fromDirection === "output" ? "input" : "output");
        if (pin) {
          const exec = pinType.kind === "exec";
          nextEdges =
            fromDirection === "output"
              ? connect(nextEdges, nodeId, handle, node.id, pin.name, exec)
              : connect(nextEdges, node.id, pin.name, nodeId, handle, exec);
        }
      }
      pushHistory();
      update(
        [...nodesRef.current.map((existing) => (existing.selected ? { ...existing, selected: false } : existing)), node],
        nextEdges,
      );
      setPalette(null);
    },
    [catalog, connect, flow, pushHistory, update],
  );

  const onNodesChange = useCallback(
    (changes: NodeChangeset<FlowNode>) => {
      if (changes.getByType("remove").length > 0) {
        pushHistory();
      }
      const positions = changes.getByType("position");
      if (positions.some((change) => change.dragging) && !draggingRef.current) {
        pushHistory();
        draggingRef.current = true;
      }
      if (positions.some((change) => change.dragging === false)) {
        draggingRef.current = false;
      }
      const dimensions = changes.getByType("dimensions");
      if (dimensions.some((change) => change.resizing) && !resizingRef.current) {
        pushHistory();
        resizingRef.current = true;
      }
      if (dimensions.some((change) => change.resizing === false)) {
        resizingRef.current = false;
      }
      const next = changes.applyTo(nodesRef.current);
      nodesRef.current = next;
      setNodes(next);
    },
    [pushHistory],
  );

  const onEdgesChange = useCallback(
    (changes: EdgeChangeset<FlowEdge>) => {
      if (changes.getByType("remove").length > 0) {
        pushHistory();
      }
      const next = changes.applyTo(edgesRef.current);
      edgesRef.current = next;
      setEdges(next);
    },
    [pushHistory],
  );

  const setLiteral = useCallback(
    (nodeId: string, pin: string, value: LiteralValue | undefined) => {
      const key = `${nodeId}:${pin}`;
      const now = Date.now();
      const last = lastLiteralRef.current;
      if (last.key !== key || now - last.time > LITERAL_COALESCE_MILLISECONDS) {
        pushHistory();
      }
      lastLiteralRef.current = { key, time: now };
      const next = nodesRef.current.map((node) => {
        if (node.id !== nodeId) {
          return node;
        }
        const literals = { ...node.data.literals };
        if (value === undefined) {
          delete literals[pin];
        } else {
          literals[pin] = value;
        }
        return { ...node, data: { ...node.data, literals } };
      });
      update(next, edgesRef.current);
    },
    [pushHistory, update],
  );

  const disconnectPin = useCallback(
    (nodeId: string, pin: string, direction: "input" | "output") => {
      const remaining = edgesRef.current.filter((edge) =>
        direction === "input"
          ? !(edge.target === nodeId && edge.targetHandle === pin)
          : !(edge.source === nodeId && edge.sourceHandle === pin),
      );
      if (remaining.length !== edgesRef.current.length) {
        pushHistory();
        update(nodesRef.current, remaining);
      }
    },
    [pushHistory, update],
  );

  const copySelection = useCallback((): boolean => {
    const selected = nodesRef.current.filter((node) => node.selected && !PROTECTED_NODES.has(node.data.nodeType));
    if (selected.length === 0) {
      return false;
    }
    const ids = new Set(selected.map((node) => node.id));
    const internal = edgesRef.current.filter((edge) => ids.has(edge.source) && ids.has(edge.target));
    const identity = new Map([...ids].map((id) => [id, id]));
    const documents = remapDocuments(flowToDocuments(selected, internal), identity, { x: 0, y: 0 });
    const payload: ClipboardPayload = {
      nanosflow: CLIPBOARD_FORMAT,
      nodes: documents,
      titles: Object.fromEntries(selected.map((node) => [node.data.nodeType, catalog.get(node.data.nodeType)?.title ?? node.data.nodeType])),
    };
    clipboard = payload;
    void navigator.clipboard?.writeText(JSON.stringify(payload)).catch(() => undefined);
    return true;
  }, [catalog]);

  const pointerInside = useCallback(() => {
    const bounds = wrapperRef.current?.getBoundingClientRect();
    const pointer = pointerRef.current;
    return Boolean(
      bounds && pointer && pointer.x >= bounds.left && pointer.x <= bounds.right && pointer.y >= bounds.top && pointer.y <= bounds.bottom,
    );
  }, []);

  const canvasCenter = useCallback(() => {
    const bounds = wrapperRef.current?.getBoundingClientRect();
    return bounds ? { x: bounds.left + bounds.width / 2 - 160, y: bounds.top + bounds.height / 3 } : { x: 300, y: 200 };
  }, []);

  const paste = useCallback(async () => {
    let source = clipboard;
    try {
      const text = await navigator.clipboard?.readText();
      const parsed: unknown = text ? JSON.parse(text) : null;
      if (isClipboardPayload(parsed)) {
        source = parsed;
      }
    } catch {
      source = clipboard;
    }
    if (!source || source.nodes.length === 0) {
      return;
    }
    const usable = source.nodes.filter((document) => !PROTECTED_NODES.has(document.type) && catalog.get(document.type));
    const skipped = source.nodes.filter((document) => !usable.includes(document) && !PROTECTED_NODES.has(document.type));
    if (skipped.length > 0) {
      const names = [...new Set(skipped.map((document) => source.titles[document.type] ?? document.type))];
      notify({
        kind: "warning",
        title: `${skipped.length} ${skipped.length === 1 ? "node was" : "nodes were"} not pasted`,
        message: `${names.join(", ")} only ${names.length === 1 ? "exists" : "exist"} where ${names.length === 1 ? "it was" : "they were"} copied from, for example a variable of another graph.`,
      });
    }
    if (usable.length === 0) {
      return;
    }
    const idMap = new Map(usable.map((document) => [document.id, generateUlid()]));
    const minX = Math.min(...usable.map((document) => document.position[0]));
    const minY = Math.min(...usable.map((document) => document.position[1]));
    const anchor = pointerInside() && pointerRef.current
      ? flow.screenToFlowPosition(pointerRef.current)
      : { x: minX + GRID * 2, y: minY + GRID * 2 };
    const offset = { x: snap(anchor.x) - minX, y: snap(anchor.y) - minY };
    const pasted = documentsToFlow(remapDocuments(usable, idMap, offset), catalog);
    pushHistory();
    update(
      [
        ...nodesRef.current.map((node) => (node.selected ? { ...node, selected: false } : node)),
        ...pasted.nodes.map((node) => ({ ...node, selected: true })),
      ],
      [...edgesRef.current.map((edge) => (edge.selected ? { ...edge, selected: false } : edge)), ...pasted.edges],
    );
  }, [catalog, flow, notify, pointerInside, pushHistory, update]);

  const setProperties = useCallback(
    (nodeId: string, patch: Record<string, unknown>) => {
      const node = nodesRef.current.find((candidate) => candidate.id === nodeId);
      if (!node) {
        return;
      }
      const properties = { ...node.data.properties, ...patch };
      const definition = catalog.definitionFor(node.data.nodeType, properties);
      const inputs = new Set(definition?.inputs.map((pin) => pin.name) ?? []);
      const outputs = new Set(definition?.outputs.map((pin) => pin.name) ?? []);
      const literals = Object.fromEntries(Object.entries(node.data.literals).filter(([pin]) => !definition || inputs.has(pin)));
      pushHistory();
      update(
        nodesRef.current.map((candidate) =>
          candidate.id === nodeId ? { ...candidate, data: { ...candidate.data, properties, literals } } : candidate,
        ),
        edgesRef.current.filter(
          (edge) =>
            !definition ||
            !(
              (edge.target === nodeId && !inputs.has(edge.targetHandle ?? "")) ||
              (edge.source === nodeId && !outputs.has(edge.sourceHandle ?? ""))
            ),
        ),
      );
    },
    [catalog, pushHistory, update],
  );

  const wireColor = useCallback(
    (nodeId: string, outputPin: string): string => {
      const node = nodesRef.current.find((candidate) => candidate.id === nodeId);
      const pin = node
        ? catalog.definitionFor(node.data.nodeType, node.data.properties)?.outputs.find((candidate) => candidate.name === outputPin)
        : undefined;
      return pin ? pinColor(catalog, resolver.pinType(nodeId, pin)) : "#8a8aa0";
    },
    [catalog, resolver],
  );

  const pinType = useCallback((nodeId: string, pin: PinDefinition) => resolver.pinType(nodeId, pin), [resolver]);

  const insertReroute = useCallback(
    (edge: FlowEdge, position: { x: number; y: number }) => {
      if (!edge.sourceHandle || !edge.targetHandle) {
        return;
      }
      const exec = edge.data?.exec ?? false;
      const knot = freshNode(exec ? EXEC_REROUTE : REROUTE, { x: Math.round(position.x - 7), y: Math.round(position.y - 7) });
      knot.selected = false;
      const remaining = edgesRef.current.filter((candidate) => candidate.id !== edge.id);
      const into = exec ? "exec" : "value";
      const outOf = exec ? "then" : "value";
      pushHistory();
      update(
        [...nodesRef.current, knot],
        [
          ...remaining,
          makeEdge(edge.source, edge.sourceHandle, knot.id, into, exec),
          makeEdge(knot.id, outOf, edge.target, edge.targetHandle, exec),
        ],
      );
    },
    [pushHistory, update],
  );

  const [editingComment, setEditingComment] = useState<string | null>(null);
  const finishCommentEditing = useCallback(() => setEditingComment(null), []);

  const wrapSelectionInComment = useCallback(() => {
    const selected = nodesRef.current.filter((node) => node.selected);
    const comment = freshNode(COMMENT, { x: 0, y: 0 });
    if (selected.length > 0) {
      const left = Math.min(...selected.map((node) => node.position.x));
      const top = Math.min(...selected.map((node) => node.position.y));
      const right = Math.max(...selected.map((node) => node.position.x + (node.measured?.width ?? node.width ?? 200)));
      const bottom = Math.max(...selected.map((node) => node.position.y + (node.measured?.height ?? node.height ?? 80)));
      comment.position = { x: snap(left - COMMENT_PADDING), y: snap(top - COMMENT_PADDING - 34) };
      comment.width = snap(right - left + COMMENT_PADDING * 2);
      comment.height = snap(bottom - top + COMMENT_PADDING * 2 + 34);
    } else {
      const anchor = flow.screenToFlowPosition(pointerInside() && pointerRef.current ? pointerRef.current : canvasCenter());
      comment.position = { x: snap(anchor.x), y: snap(anchor.y) };
    }
    pushHistory();
    update([...nodesRef.current.map((node) => (node.selected ? { ...node, selected: false } : node)), comment], edgesRef.current);
    setEditingComment(comment.id);
  }, [canvasCenter, flow, pointerInside, pushHistory, update]);

  const toggleBreakpoint = useCallback(() => {
    const selected = nodesRef.current.filter((node) => node.selected);
    if (selected.length !== 1) {
      return;
    }
    const [node] = selected;
    const definition = catalog.definitionFor(node.data.nodeType, node.data.properties);
    if (!definition?.inputs.some((pin) => pin.type.kind === "exec")) {
      notify({ kind: "warning", title: "Breakpoints need an execution input", message: "Pure nodes run as part of another node. Put the breakpoint on the node that uses the value." });
      return;
    }
    setProperties(node.id, { breakpoint: node.data.properties.breakpoint === true ? undefined : true });
  }, [catalog, notify, setProperties]);

  const resizingRef = useRef(false);
  const commentDragRef = useRef<{ id: string; start: { x: number; y: number }; children: Map<string, { x: number; y: number }> } | null>(null);

  const onNodeDragStart = useCallback((_: unknown, node: FlowNode) => {
    if (node.type !== "comment") {
      commentDragRef.current = null;
      return;
    }
    const width = node.width ?? node.measured?.width ?? COMMENT_SIZE.width;
    const height = node.height ?? node.measured?.height ?? COMMENT_SIZE.height;
    const children = new Map<string, { x: number; y: number }>();
    for (const candidate of nodesRef.current) {
      if (candidate.id === node.id || candidate.selected) {
        continue;
      }
      const candidateWidth = candidate.measured?.width ?? candidate.width ?? 0;
      const candidateHeight = candidate.measured?.height ?? candidate.height ?? 0;
      const inside =
        candidate.position.x >= node.position.x &&
        candidate.position.y >= node.position.y &&
        candidate.position.x + candidateWidth <= node.position.x + width &&
        candidate.position.y + candidateHeight <= node.position.y + height;
      if (inside) {
        children.set(candidate.id, candidate.position);
      }
    }
    commentDragRef.current = { id: node.id, start: node.position, children };
  }, []);

  const onNodeDrag = useCallback((_: unknown, node: FlowNode) => {
    const drag = commentDragRef.current;
    if (!drag || drag.id !== node.id || drag.children.size === 0) {
      return;
    }
    const dx = node.position.x - drag.start.x;
    const dy = node.position.y - drag.start.y;
    const next = nodesRef.current.map((candidate) => {
      const start = drag.children.get(candidate.id);
      return start ? { ...candidate, position: { x: start.x + dx, y: start.y + dy } } : candidate;
    });
    nodesRef.current = next;
    setNodes(next);
  }, []);

  const onNodeDragStop = useCallback(() => {
    commentDragRef.current = null;
  }, []);

  const deleteSelection = useCallback(() => {
    const removed = new Set(
      nodesRef.current.filter((node) => node.selected && node.deletable !== false).map((node) => node.id),
    );
    if (removed.size === 0) {
      return;
    }
    pushHistory();
    update(
      nodesRef.current.filter((node) => !removed.has(node.id)),
      edgesRef.current.filter((edge) => !removed.has(edge.source) && !removed.has(edge.target)),
    );
  }, [pushHistory, update]);

  const [nodeMenu, setNodeMenu] = useState<{ x: number; y: number } | null>(null);
  const closeNodeMenu = useCallback(() => setNodeMenu(null), []);

  const collapseSelection = useCallback(async () => {
    const selected = nodesRef.current.filter((node) => node.selected && node.type !== "comment").map((node) => node.id);
    if (selected.length === 0) {
      return;
    }
    try {
      await save();
      const problem = await api.checkCollapse(apiChannel, packagePath, graph.path, selected);
      if (!problem) {
        onCollapseSelection(selected);
        return;
      }
      notify({ kind: "warning", title: "This selection cannot become a function", message: `${problem.message} ${problem.hint}` });
      if (problem.nodes.length > 0) {
        const marked = new Set(problem.nodes);
        update(
          nodesRef.current.map((node) => ({ ...node, selected: marked.has(node.id) })),
          edgesRef.current,
        );
      }
    } catch (error) {
      reportError(toCommandError(error));
    }
  }, [save, apiChannel, packagePath, graph.path, onCollapseSelection, notify, update, reportError]);

  const openNodeMenu = useCallback((event: { preventDefault: () => void; clientX: number; clientY: number }, node?: FlowNode) => {
    event.preventDefault();
    const press = rightPressRef.current;
    rightPressRef.current = null;
    if (press && Math.hypot(press.x - event.clientX, press.y - event.clientY) > 4) {
      return;
    }
    if (node && !node.selected) {
      update(
        nodesRef.current.map((candidate) => ({ ...candidate, selected: candidate.id === node.id })),
        edgesRef.current,
      );
    }
    setNodeMenu({ x: event.clientX, y: event.clientY });
  }, [update]);

  const isMethodGraph = graph.kind === "class" || Boolean(graph.owner);
  const menuSelection = nodeMenu ? nodesRef.current.filter((node) => node.selected) : [];
  const menuNode = menuSelection.length === 1 ? menuSelection[0] : null;
  const menuTarget = menuNode
    ? menuNode.data.nodeType === FUNCTION_ENTRY || menuNode.data.nodeType === MACRO_INPUTS
      ? graph.id
      : usageTarget(menuNode.data.nodeType)
    : null;
  const menuDefinition = menuNode ? catalog.definitionFor(menuNode.data.nodeType, menuNode.data.properties) : undefined;
  const isWatchable = Boolean(menuDefinition?.inputs.some((pin) => pin.type.kind === "exec"));
  const isWatched = menuNode?.data.properties.watch === true;
  const hasBreakpoint = menuNode?.data.properties.breakpoint === true;
  const nodeMenuItems: MenuItem[] = [
    ...(menuNode && isWatchable
      ? [
          {
            label: hasBreakpoint ? "Remove breakpoint (F9)" : "Add breakpoint (F9)",
            icon: <CircleDot size={14} />,
            onSelect: () => setProperties(menuNode.id, { breakpoint: hasBreakpoint ? undefined : true }),
          },
        ]
      : []),
    ...(menuNode && isWatchable
      ? [
          {
            label: isWatched ? "Stop watching" : "Watch values",
            icon: isWatched ? <EyeOff size={14} /> : <Eye size={14} />,
            onSelect: () => setProperties(menuNode.id, { watch: isWatched ? undefined : true }),
          },
        ]
      : []),
    ...(menuNode && menuTarget
      ? [
          {
            label: "Find usages",
            icon: <Search size={14} />,
            onSelect: () =>
              onFindUsages(menuTarget, menuNode.data.nodeType === FUNCTION_ENTRY || menuNode.data.nodeType === MACRO_INPUTS ? graph.name : (catalog.get(menuNode.data.nodeType)?.title ?? graph.name)),
          },
        ]
      : []),
    {
      label: isMethodGraph ? "Collapse to method" : "Collapse to function",
      icon: <SquareFunction size={14} />,
      separatorBefore: Boolean(menuNode && (menuTarget || isWatchable)),
      onSelect: () => void collapseSelection(),
    },
    { label: "Comment around selection", icon: <MessageSquare size={14} />, onSelect: () => wrapSelectionInComment() },
    {
      label: "Duplicate",
      icon: <Copy size={14} />,
      onSelect: () => {
        if (copySelection()) {
          void paste();
        }
      },
    },
    { label: "Delete", icon: <Trash2 size={14} />, danger: true, separatorBefore: true, onSelect: () => deleteSelection() },
  ];

  const placeNode = useCallback(
    (nodeType: string, screen: { x: number; y: number }) => {
      const anchor = flow.screenToFlowPosition(screen);
      const node = freshNode(nodeType, { x: snap(anchor.x), y: snap(anchor.y - 20) });
      pushHistory();
      update(
        [...nodesRef.current.map((existing) => (existing.selected ? { ...existing, selected: false } : existing)), node],
        edgesRef.current,
      );
    },
    [flow, pushHistory, update],
  );

  const startPlacingVariable = useCallback(
    (variable: VariableDocument, mode: PlaceMode, event: React.PointerEvent) => {
      event.preventDefault();
      const start = { x: event.clientX, y: event.clientY };
      const label = `${mode === "get" ? "Get" : "Set"} ${variable.name}`;
      let moved = false;
      const onMove = (moveEvent: PointerEvent) => {
        if (Math.hypot(moveEvent.clientX - start.x, moveEvent.clientY - start.y) > 4) {
          moved = true;
        }
        if (moved) {
          setPlacing({ label, x: moveEvent.clientX, y: moveEvent.clientY });
        }
      };
      const onUp = (upEvent: PointerEvent) => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        setPlacing(null);
        const bounds = wrapperRef.current?.getBoundingClientRect();
        const inside =
          bounds &&
          upEvent.clientX >= bounds.left &&
          upEvent.clientX <= bounds.right &&
          upEvent.clientY >= bounds.top &&
          upEvent.clientY <= bounds.bottom;
        if (moved && !inside) {
          return;
        }
        const screen = moved ? { x: upEvent.clientX, y: upEvent.clientY } : canvasCenter();
        void saveVariables().then((saved) => {
          if (saved) {
            placeNode(`variable/${mode}/${variable.id}`, screen);
          }
        });
      };
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    },
    [canvasCenter, placeNode, saveVariables],
  );

  const conflictCount = loadErrors.filter((error) => error.hasConflictMarkers).length;

  const openMerge = useCallback(async () => {
    try {
      await save();
      setConflicts(await api.graphConflicts(projectFile, graph.path));
    } catch (error) {
      reportError(toCommandError(error));
    }
  }, [save, projectFile, graph.path, reportError]);

  const resolveConflicts = useCallback(
    async (targets: FileConflict[], choice: ConflictChoice) => {
      setMergeBusy(true);
      try {
        let remaining: FileConflict[] = conflicts ?? [];
        for (const target of targets) {
          remaining = await api.resolveConflict(projectFile, graph.path, target.file, choice);
        }
        setConflicts(remaining);
        setReloadToken((token) => token + 1);
      } catch (error) {
        reportError(toCommandError(error));
      } finally {
        setMergeBusy(false);
      }
    },
    [conflicts, projectFile, graph.path, reportError],
  );

  const toggleChanges = useCallback(async () => {
    if (committed) {
      setCommitted(null);
      return;
    }
    setChangesLoading(true);
    try {
      const result = await api.committedGraph(projectFile, graph.path);
      if (result) {
        setCommitted(result);
      } else {
        notify({
          kind: "warning",
          title: "No commit to compare with",
          message: "This graph is not in a Git repository yet, or nothing has been committed. Commit once to see changes here.",
        });
      }
    } catch (error) {
      reportError(toCommandError(error));
    } finally {
      setChangesLoading(false);
    }
  }, [committed, projectFile, graph.path, notify, reportError]);

  const changes = useMemo(() => (committed ? computeChanges(documents, committed.nodes) : null), [committed, documents]);

  const applyDocuments = useCallback(
    (next: NodeDocument[]) => {
      pushHistory();
      const flowDocuments = documentsToFlow(next, catalog);
      update(withEntryProtection(flowDocuments.nodes), flowDocuments.edges);
    },
    [catalog, pushHistory, update],
  );

  const restoreCommitted = useCallback(
    (original: NodeDocument) => {
      applyDocuments([...latestRef.current.documents.filter((node) => node.id !== original.id), original]);
    },
    [applyDocuments],
  );

  const mergeRemoteNodes = useCallback(
    (remoteDocuments: NodeDocument[], undoable: boolean) => {
      const toFlow = (documents: NodeDocument[]) => {
        const converted = documentsToFlow(documents, catalog);
        return { nodes: withEntryProtection(converted.nodes), edges: converted.edges };
      };
      const remoteFlow = toFlow(remoteDocuments);
      const normalized = flowToDocuments(remoteFlow.nodes, remoteFlow.edges);
      const remote = new Map(normalized.map((node) => [node.id, { node, text: serializeDocument(node) }]));
      const saved = savedRef.current;
      const upserts = [...remote.values()].filter((entry) => saved.get(entry.node.id) !== entry.text);
      const removals = [...saved.keys()].filter((id) => !remote.has(id));
      if (upserts.length === 0 && removals.length === 0) {
        return;
      }
      const patch = (snapshot: Snapshot): Snapshot => {
        const byId = new Map(flowToDocuments(snapshot.nodes, snapshot.edges).map((node) => [node.id, node]));
        for (const id of removals) {
          byId.delete(id);
        }
        for (const entry of upserts) {
          byId.set(entry.node.id, entry.node);
        }
        const selected = new Set(snapshot.nodes.filter((node) => node.selected).map((node) => node.id));
        const patched = toFlow([...byId.values()]);
        return { nodes: patched.nodes.map((node) => (selected.has(node.id) ? { ...node, selected: true } : node)), edges: patched.edges };
      };
      const history = historyRef.current;
      if (undoable) {
        history.past.push({ nodes: nodesRef.current, edges: edgesRef.current });
        if (history.past.length > HISTORY_LIMIT) {
          history.past.shift();
        }
        history.future = [];
        syncHistorySize();
      } else {
        history.past = history.past.map(patch);
        history.future = history.future.map(patch);
      }
      const nextSaved = new Map(saved);
      for (const id of removals) {
        nextSaved.delete(id);
      }
      for (const entry of upserts) {
        nextSaved.set(entry.node.id, entry.text);
      }
      savedRef.current = nextSaved;
      const current = patch({ nodes: nodesRef.current, edges: edgesRef.current });
      update(current.nodes, current.edges);
      setLuaRevision((value) => value + 1);
    },
    [catalog, update, syncHistorySize],
  );

  const appliedRemoteRef = useRef(remoteChange?.revision ?? 0);
  useEffect(() => {
    if (!remoteChange || status !== "ready" || appliedRemoteRef.current === remoteChange.revision) {
      return;
    }
    appliedRemoteRef.current = remoteChange.revision;
    const base = `${normalizePath(graph.path)}/`;
    const mine = remoteChange.paths.map(normalizePath).filter((path) => path.startsWith(base));
    if (mine.length === 0) {
      return;
    }
    if (mine.some((path) => !path.startsWith(`${base}nodes/`))) {
      setReloadToken((value) => value + 1);
      return;
    }
    let cancelled = false;
    void saveChainRef.current
      .then(() => api.loadGraph(graph.path))
      .then((document) => {
        if (!cancelled) {
          mergeRemoteNodes(document.nodes, remoteChange.undoable ?? false);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [remoteChange, status, graph.path, mergeRemoteNodes]);

  const presenceRef = useRef({ last: 0, x: 0, y: 0 });
  const selectionKey = nodes
    .filter((node) => node.selected)
    .map((node) => node.id)
    .join(",");
  const reportPresence = useCallback(
    (point: { x: number; y: number } | null) => {
      if (!onPresence) {
        return;
      }
      if (point) {
        presenceRef.current = { ...presenceRef.current, x: point.x, y: point.y };
      }
      const now = performance.now();
      if (point && now - presenceRef.current.last < PRESENCE_INTERVAL) {
        return;
      }
      presenceRef.current.last = now;
      onPresence({
        x: presenceRef.current.x,
        y: presenceRef.current.y,
        selection: nodesRef.current.filter((node) => node.selected).map((node) => node.id),
      });
    },
    [onPresence],
  );

  useEffect(() => {
    reportPresence(null);
  }, [selectionKey, reportPresence]);

  const remoteSelection = useMemo(() => {
    const selected: Record<string, { color: string; name: string }> = {};
    for (const peer of peers) {
      for (const id of peer.selection) {
        selected[id] = { color: peer.color, name: peer.name };
      }
    }
    return selected;
  }, [peers]);

  const nodeTitle = useCallback((nodeType: string) => catalog.get(nodeType)?.title ?? nodeType, [catalog]);
  const linkedTitle = useCallback(
    (nodeId: string) => {
      const node = nodesRef.current.find((candidate) => candidate.id === nodeId);
      return node ? nodeTitle(node.data.nodeType) : "a node that is not in this graph";
    },
    [nodeTitle],
  );

  const focusNode = useCallback(
    (nodeId: string) => {
      const node = flow.getNode(nodeId);
      if (!node) {
        return;
      }
      update(
        nodesRef.current.map((candidate) => ({ ...candidate, selected: candidate.id === nodeId })),
        edgesRef.current,
      );
      const width = node.measured?.width ?? 200;
      const height = node.measured?.height ?? 80;
      void flow.setCenter(node.position.x + width / 2, node.position.y + height / 2, {
        zoom: Math.max(flow.getZoom(), 1),
        duration: 300,
      });
    },
    [flow, update],
  );

  useEffect(() => {
    if (status !== "ready" || !focusRequest || focusRequest.graphId !== graph.id) {
      return;
    }
    const timer = window.setTimeout(() => focusNode(focusRequest.node), 60);
    return () => window.clearTimeout(timer);
  }, [status, focusRequest, graph.id, focusNode]);

  const fitView = useCallback(() => {
    const selected = nodesRef.current.filter((node) => node.selected);
    void flow.fitView({ nodes: selected.length > 0 ? selected : undefined, padding: 0.25, maxZoom: 1.2, duration: 300 });
  }, [flow]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (palette || timelineNode || status !== "ready") {
        return;
      }
      const target = event.target as HTMLElement | null;
      const isTextField =
        target?.closest("textarea, [contenteditable='true']") ||
        (target instanceof HTMLInputElement && !["checkbox", "radio", "button"].includes(target.type));
      if (isTextField) {
        return;
      }
      const isControl = target?.closest("input, select, button") && !wrapperRef.current?.contains(target);
      if (isControl && !pointerInside()) {
        return;
      }
      const modifier = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      if (modifier && key === "z" && !event.shiftKey) {
        event.preventDefault();
        undo();
      } else if (modifier && (key === "y" || (key === "z" && event.shiftKey))) {
        event.preventDefault();
        redo();
      } else if (modifier && key === "c") {
        copySelection();
      } else if (modifier && key === "x") {
        if (copySelection()) {
          deleteSelection();
        }
      } else if (modifier && key === "v") {
        event.preventDefault();
        void paste();
      } else if (modifier && key === "d") {
        event.preventDefault();
        if (copySelection()) {
          void paste();
        }
      } else if (!modifier && key === "c" && !event.shiftKey && !event.altKey) {
        event.preventDefault();
        wrapSelectionInComment();
      } else if (modifier && key === "a") {
        event.preventDefault();
        update(
          nodesRef.current.map((node) => ({ ...node, selected: true })),
          edgesRef.current,
        );
      } else if (modifier && key === "s") {
        event.preventDefault();
        void save();
      } else if (!modifier && (event.key === " " || event.key === "Tab")) {
        event.preventDefault();
        openPalette(pointerInside() && pointerRef.current ? pointerRef.current : canvasCenter());
      } else if (!modifier && key === "f") {
        event.preventDefault();
        fitView();
      } else if (!modifier && event.key === "F9") {
        event.preventDefault();
        toggleBreakpoint();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [palette, timelineNode, status, undo, redo, copySelection, deleteSelection, paste, save, openPalette, pointerInside, canvasCenter, fitView, update, wrapSelectionInComment, toggleBreakpoint]);

  useEffect(() => {
    if (status !== "ready") {
      return;
    }
    let cancelled = false;
    setLuaPreview((current) => ({ ...current, loading: true }));
    const timer = window.setTimeout(() => {
      api
        .previewLua(apiChannel, packagePath, graph.id)
        .then((data) => {
          if (!cancelled) {
            setLuaPreview({ data, error: null, loading: false });
          }
        })
        .catch((error) => {
          if (!cancelled) {
            setLuaPreview({ data: null, error: toCommandError(error), loading: false });
          }
        });
    }, LUA_REFRESH_DELAY);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [status, apiChannel, packagePath, graph.id, luaRevision]);

  const allDiagnostics = useMemo(() => {
    const nodeIds = new Set(nodes.map((node) => node.id));
    const buildOnly: Diagnostic[] = (luaPreview.data?.problems ?? [])
      .filter((problem) => problem.code !== "damaged-file" && (!problem.node || nodeIds.has(problem.node)))
      .filter(
        (problem) =>
          !diagnostics.some(
            (diagnostic) => diagnostic.code === problem.code && diagnostic.node === problem.node && diagnostic.message === problem.message,
          ),
      )
      .map((problem) => ({
        severity: problem.severity,
        code: problem.code,
        node: problem.node,
        pin: problem.pin,
        message: problem.message,
        hint: problem.hint,
      }));
    return buildOnly.length === 0 ? diagnostics : [...diagnostics, ...buildOnly];
  }, [diagnostics, luaPreview.data, nodes]);

  const diagnosticsByNode = useMemo(() => {
    const map = new Map<string, Diagnostic[]>();
    for (const diagnostic of allDiagnostics) {
      if (!diagnostic.node) {
        continue;
      }
      const list = map.get(diagnostic.node) ?? [];
      list.push(diagnostic);
      map.set(diagnostic.node, list);
    }
    return map;
  }, [allDiagnostics]);

  const connectedPins = useMemo(() => {
    const set = new Set<string>();
    for (const edge of edges) {
      set.add(pinKey(edge.source, "output", edge.sourceHandle ?? ""));
      set.add(pinKey(edge.target, "input", edge.targetHandle ?? ""));
    }
    return set;
  }, [edges]);

  const editorApi = useMemo<EditorApi>(
    () => ({
      catalog,
      context: graph.context,
      diagnosticsByNode,
      connectedPins,
      setLiteral,
      disconnectPin,
      setProperties,
      editTimeline: setTimelineNode,
      wireColor,
      pinType,
      editingComment,
      finishCommentEditing,
      runtime,
      changes: changes?.byNode ?? NO_CHANGES,
      remoteSelection,
    }),
    [catalog, graph.context, diagnosticsByNode, connectedPins, setLiteral, disconnectPin, setProperties, wireColor, pinType, editingComment, finishCommentEditing, runtime, changes, remoteSelection],
  );

  const selectedNodes = nodes.filter((node) => node.selected);
  const selectedDefinition =
    selectedNodes.length === 1 ? catalog.definitionFor(selectedNodes[0].data.nodeType, selectedNodes[0].data.properties) : undefined;
  const errorCount = allDiagnostics.filter((diagnostic) => diagnostic.severity === "error").length + loadErrors.length;

  if (status === "failed") {
    return (
      <div className={styles.failure}>
        <CircleAlert size={28} />
        <h2>{failure?.title ?? "The graph could not be opened"}</h2>
        <p>{failure?.message}</p>
      </div>
    );
  }

  return (
    <EditorContext.Provider value={editorApi}>
      {conflicts && (
        <MergePanel
          conflicts={conflicts}
          nodeTitle={nodeTitle}
          linkedTitle={linkedTitle}
          busy={mergeBusy}
          onResolve={(conflict, choice) => void resolveConflicts([conflict], choice)}
          onResolveAll={(choice) => void resolveConflicts(conflicts, choice)}
          onClose={() => setConflicts(null)}
        />
      )}
      {timelineNode && nodes.some((node) => node.id === timelineNode) && (
        <TimelineEditor
          properties={nodes.find((node) => node.id === timelineNode)?.data.properties ?? {}}
          onCommit={(patch) => setProperties(timelineNode, patch)}
          onClose={() => setTimelineNode(null)}
        />
      )}
      <div className={styles.editor}>
        <div className={styles.toolbar}>
          <span className={styles.breadcrumb}>
            <span className={styles.packageName}>{packageFolder}</span>
            <span className={styles.separator}>/</span>
            <span className={styles.graphName}>{graph.name}</span>
            <span className={styles.contextChip} data-context={graph.context}>
              {graph.context}
            </span>
          </span>
          <span className={styles.spacer} />
          <span className={styles.saveState} data-state={saveState} aria-live="polite">
            {saveState === "saving" && <LoaderCircle size={13} className={styles.spin} />}
            {saveState === "saved" && <Check size={13} />}
            {saveState === "failed" && <CloudOff size={13} />}
            {saveState === "saving" ? "Saving" : saveState === "saved" ? "Saved" : "Not saved"}
          </span>
          <Button variant="ghost" size="small" icon={<Undo2 size={15} />} onClick={undo} disabled={historySize.past === 0} title="Undo (Ctrl+Z)" />
          <Button variant="ghost" size="small" icon={<Redo2 size={15} />} onClick={redo} disabled={historySize.future === 0} title="Redo (Ctrl+Y)" />
          <Button variant="ghost" size="small" icon={<Maximize size={14} />} onClick={fitView} title="Fit to view (F)" />
          <Button variant="secondary" size="small" icon={<Plus size={15} />} onClick={() => openPalette(canvasCenter())} title="Add node (Space)">
            Add node
          </Button>
          <Button
            variant="ghost"
            size="small"
            icon={changesLoading ? <LoaderCircle size={15} className={styles.spin} /> : <GitCompareArrows size={15} />}
            onClick={() => void toggleChanges()}
            title={committed ? "Hide the changes since the last commit" : "Show what changed since the last Git commit"}
            aria-pressed={Boolean(committed)}
            className={committed ? styles.toggleActive : undefined}
          >
            Changes
          </Button>
          <Button
            variant="ghost"
            size="small"
            icon={<FileCode size={15} />}
            onClick={() => onSidePanelChange(sidePanel === "lua" ? null : "lua")}
            title={sidePanel === "lua" ? "Hide the generated Lua" : "Show the generated Lua"}
            aria-pressed={sidePanel === "lua"}
            className={sidePanel === "lua" ? styles.toggleActive : undefined}
          >
            Lua
          </Button>
          <Button
            variant="ghost"
            size="small"
            icon={<PanelRight size={15} />}
            onClick={() => onSidePanelChange(sidePanel === "details" ? null : "details")}
            title={sidePanel === "details" ? "Hide details" : "Show details"}
            aria-pressed={sidePanel === "details"}
            className={sidePanel === "details" ? styles.toggleActive : undefined}
          />
        </div>

        <div className={styles.workspace} data-panel={sidePanel ?? "none"}>
          <div
            ref={wrapperRef}
            className={styles.canvas}
            data-conflicts={conflictCount > 0}
            onPointerMove={(event) => {
              pointerRef.current = { x: event.clientX, y: event.clientY };
              if (onPresence) {
                reportPresence(flow.screenToFlowPosition({ x: event.clientX, y: event.clientY }));
              }
            }}
            onPointerDownCapture={(event) => {
              const active = document.activeElement;
              if (active instanceof HTMLElement && !wrapperRef.current?.contains(active)) {
                active.blur();
              }
              if (event.button === 2) {
                rightPressRef.current = { x: event.clientX, y: event.clientY };
              }
            }}
          >
            {conflictCount > 0 && (
            <div className={styles.conflictBanner} role="alert">
              <GitMerge size={15} />
              <span>
                {conflictCount} {conflictCount === 1 ? "file has" : "files have"} merge conflicts. Until they are resolved,{" "}
                {loadErrors.some((error) => error.file === "graph.toml") ? "your graph settings are used and " : ""}the conflicting nodes are hidden.
              </span>
              <Button variant="primary" size="small" onClick={() => void openMerge()}>
                Resolve
              </Button>
            </div>
          )}
          {changes && committed && (
            <div className={styles.changesPanel}>
              <div className={styles.changesHeader}>
                <GitCompareArrows size={14} />
                <span>
                  Since <code>{committed.revision}</code> {committed.summary}
                </span>
              </div>
              {Object.keys(changes.byNode).length === 0 && changes.removed.length === 0 ? (
                <p className={styles.changesEmpty}>No changes since this commit.</p>
              ) : (
                <ul className={styles.changesList}>
                  {Object.entries(changes.byNode).map(([nodeId, kind]) => {
                    const node = nodes.find((candidate) => candidate.id === nodeId);
                    const original = changes.committed.get(nodeId);
                    return (
                      <li key={nodeId} data-change={kind}>
                        <button type="button" className={styles.changeTitle} onClick={() => focusNode(nodeId)}>
                          <span className={styles.changeKind}>{kind === "added" ? "New" : kind === "changed" ? "Changed" : "Moved"}</span>
                          {node ? nodeTitle(node.data.nodeType) : nodeId}
                        </button>
                        {original && (
                          <button type="button" className={styles.changeAction} title="Put this node back to the committed version" onClick={() => restoreCommitted(original)}>
                            <RotateCcw size={12} />
                          </button>
                        )}
                      </li>
                    );
                  })}
                  {changes.removed.map((original) => (
                    <li key={original.id} data-change="removed">
                      <span className={styles.changeTitle}>
                        <span className={styles.changeKind}>Removed</span>
                        {nodeTitle(original.type)}
                      </span>
                      <button type="button" className={styles.changeAction} title="Bring this node back" onClick={() => restoreCommitted(original)}>
                        <RotateCcw size={12} />
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
            {status === "loading" ? (
              <div className={styles.loading}>
                <LoaderCircle size={22} className={styles.spin} />
                Loading graph
              </div>
            ) : (
              <ReactFlow<FlowNode, FlowEdge>
                nodes={nodes}
                edges={edges}
                nodeTypes={NODE_TYPES}
                edgeTypes={EDGE_TYPES}
                onNodesChange={onNodesChange}
                onNodeDragStart={onNodeDragStart}
                onNodeDrag={onNodeDrag}
                onNodeDragStop={onNodeDragStop}
                onEdgeDoubleClick={(event, edge) => insertReroute(edge, flow.screenToFlowPosition({ x: event.clientX, y: event.clientY }))}
                onEdgesChange={onEdgesChange}
                onConnect={onConnect}
                onConnectEnd={onConnectEnd}
                isValidConnection={isValidConnection}
                connectionLineComponent={ConnectionLine}
                connectionMode={ConnectionMode.Strict}
                connectionRadius={22}
                onNodeContextMenu={(event, node) => openNodeMenu(event, node)}
                onSelectionContextMenu={(event) => openNodeMenu(event)}
                onPaneContextMenu={(event) => {
                  event.preventDefault();
                  const press = rightPressRef.current;
                  rightPressRef.current = null;
                  if (press && Math.hypot(press.x - event.clientX, press.y - event.clientY) > 4) {
                    return;
                  }
                  openPalette({ x: event.clientX, y: event.clientY });
                }}
                deleteKeyCode={timelineNode ? null : ["Delete", "Backspace"]}
                multiSelectionKeyCode={["Shift", "Control", "Meta"]}
                selectionOnDrag
                panOnDrag={[1, 2]}
                snapToGrid
                snapGrid={[GRID, GRID]}
                minZoom={0.15}
                maxZoom={2}
                zoomOnDoubleClick={false}
                fitView
                fitViewOptions={{ padding: 0.3, maxZoom: 1 }}
                onlyRenderVisibleElements
                attributionPosition="bottom-left"
              >
                <Background id="minor" variant={BackgroundVariant.Lines} gap={GRID} color="rgba(255,255,255,0.025)" />
                <Background id="major" variant={BackgroundVariant.Lines} gap={GRID * 8} color="rgba(255,255,255,0.06)" />
                <MiniMap
                  pannable
                  zoomable
                  className={styles.minimap}
                  maskColor="rgba(8, 8, 14, 0.7)"
                  nodeColor={(node) => {
                    const definition = catalog.get((node as FlowNode).data.nodeType);
                    return definition ? definitionColor(definition) : "#7f1d1d";
                  }}
                />
                <ConnectionHint hint={connectionHint} />
                <RemoteCursors peers={peers} />
              </ReactFlow>
            )}

            {status === "ready" && nodes.length === 0 && (
              <div className={styles.emptyHint}>
                <MousePointerClick size={22} />
                <p>
                  <strong>This graph is empty.</strong> Right-click or press <kbd>Space</kbd> to add a node, for example an
                  event like{" "}
                  <em>{graph.kind === "class" ? `On ${graph.name} Spawn` : "On Package Load"}</em>.
                </p>
              </div>
            )}
          </div>

          {sidePanel === "lua" && (
            <aside className={styles.luaPanel}>
              <LuaPreview
                state={luaPreview}
                graphKind={graph.kind}
                onSelectProblem={(problem: BuildProblem) => problem.node && focusNode(problem.node)}
              />
            </aside>
          )}

          {sidePanel === "details" && (
            <aside className={styles.details}>
              {selectedDefinition && !PROTECTED_NODES.has(selectedNodes[0].data.nodeType) ? (
                <NodeDocs definition={selectedDefinition} catalog={catalog} context={graph.context} />
              ) : (
                <div className={styles.sideStack}>
                  {functionSignature ? (
                    <FunctionSignatureEditor
                      variant={graph.kind === "macro" ? "macro" : graph.owner ? "method" : "function"}
                      libraryName={libraryName(packageFolder)}
                      name={graph.name}
                      catalog={catalog}
                      signature={functionSignature}
                      issues={signatureIssues}
                      onChange={changeSignature}
                    />
                  ) : graph.kind === "class" && classParent !== null ? (
                    <ClassSettings
                      name={graph.name}
                      context={graph.context}
                      parent={classParent}
                      description={classDescription}
                      apiParents={catalog.apiClassNames().sort()}
                      projectParents={catalog.projectClassNames().filter((name) => name !== graph.name).sort()}
                      issue={classIssue}
                      interfaces={classInterfaces}
                      availableInterfaces={projectInterfaces}
                      onInterfacesChange={changeClassInterfaces}
                      exported={classExported}
                      onExportedChange={changeClassExported}
                      onParentChange={changeClassParent}
                      onDescriptionChange={(description) => {
                        classDescriptionDirtyRef.current = true;
                        setClassDescription(description);
                      }}
                    />
                  ) : (
                    <div className={styles.graphInfo}>
                      <h3>{graph.name}</h3>
                      <p className={styles.infoLine}>
                        <span className={styles.contextChip} data-context={graph.context}>
                          {graph.context}
                        </span>
                        {nodes.length} {nodes.length === 1 ? "node" : "nodes"}, {edges.length} {edges.length === 1 ? "wire" : "wires"}
                      </p>
                      {selectedNodes.length > 1 && <p className={styles.infoLine}>{selectedNodes.length} nodes selected</p>}
                      <p className={styles.infoText}>
                        {errorCount > 0
                          ? `${errorCount} ${errorCount === 1 ? "error needs" : "errors need"} attention. Open Problems below to jump to them.`
                          : "Select a node to see its documentation, inputs and outputs."}
                      </p>
                    </div>
                  )}
                  <VariablesPanel
                    title={graph.kind === "class" ? "Member variables" : "Variables"}
                    hint={
                      graph.kind === "class"
                        ? "Every instance gets its own copy. Other graphs in the package can read and change them through a Target."
                        : graph.kind === "function"
                          ? "Local variables start at their default value every time the function runs."
                          : graph.kind === "macro"
                            ? "Every place that uses this macro gets its own copy. It behaves like a variable of that graph."
                            : "Variables keep their value while the package runs and are only visible in this graph."
                    }
                    catalog={catalog}
                    variables={variables}
                    issues={variableIssues}
                    allowSync={graph.kind === "class"}
                    onChange={changeVariables}
                    onPlace={startPlacingVariable}
                    onFindUsages={(variable) => onFindUsages(variable.id, variable.name)}
                  />
                  {graph.kind === "event-graph" && (
                    <div className={styles.shortcuts}>
                      {[
                        ["Space", "Add node"],
                        ["Right drag", "Pan"],
                        ["Left drag", "Select"],
                        ["Alt + click pin", "Disconnect"],
                        ["Ctrl + D", "Duplicate"],
                        ["C", "Comment around selection"],
                        ["Double-click wire", "Add reroute point"],
                        ["Ctrl + Z / Y", "Undo / Redo"],
                        ["F", "Fit to view"],
                      ].map(([keys, action]) => (
                        <div key={keys} className={styles.shortcut}>
                          <kbd>{keys}</kbd>
                          <span>{action}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </aside>
          )}
        </div>

        <ProblemsPanel
          diagnostics={allDiagnostics}
          loadErrors={loadErrors}
          onSelect={(diagnostic) => diagnostic.node && focusNode(diagnostic.node)}
        />

        {placing && (
          <div className={styles.placeGhost} style={{ left: placing.x + 12, top: placing.y + 8 }}>
            {placing.label}
          </div>
        )}

        {nodeMenu && <ContextMenu position={nodeMenu} items={nodeMenuItems} onClose={closeNodeMenu} />}

        {palette && (
          <NodePalette
            catalog={catalog}
            context={graph.context}
            screenPosition={palette.screen}
            filter={palette.filter}
            onPick={(definition) => addNode(definition, palette)}
            onClose={() => setPalette(null)}
          />
        )}
      </div>
    </EditorContext.Provider>
  );
}

