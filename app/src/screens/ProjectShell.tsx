import { ask } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  Activity,
  Bot,
  ChevronLeft,
  CircleHelp,
  Command,
  CircleAlert,
  CircleCheck,
  FolderSearch,
  Hammer,
  LoaderCircle,
  Play,
  RotateCw,
  Search,
  Server,
  Users,
  Settings,
  Square,
  TriangleAlert,
  Workflow,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { useToasts } from "../components/Toasts";
import { NodeCatalog } from "../graph/catalog";
import { DefinitionEditor } from "../graph/DefinitionEditor";
import { InterfaceEditor } from "../graph/InterfaceEditor";
import { GraphEditor, normalizePath, type FocusRequest, type RemoteChange, type SidePanel } from "../graph/GraphEditor";
import { peerColor, type RemotePeer } from "../graph/RemoteCursors";
import type { BreakpointHit, BuildProblem, DebuggerMessage, LogEntry, ResumeMode, StackFrame, RuntimeMark, SearchHit, CommandError, EntryLocation, GraphKind, GraphSummary, PackageSummary, AgentFilesMessage, AgentOpenMessage, McpStatus, Presence, ProjectView, SessionFilesMessage, SessionStatus, TestServerMessage, TestServerStatus } from "../types";
import { BuildPanel, type BuildState } from "./BuildPanel";
import { ProjectSettings } from "./ProjectSettings";
import { EntryDialog, type EntryDialogMode } from "./EntryDialog";
import { ProjectTree, type CreateRequest, type TreeEntry } from "./ProjectTree";
import { SearchPanel, type SearchRequest } from "./SearchPanel";
import { RuntimePanel } from "./RuntimePanel";
import { DebugBar } from "./DebugBar";
import { sessionState } from "../session/SessionPanel";
import { agentActive } from "../settings/useMcp";
import type { SettingsSection } from "../settings/SettingsPage";
import { modifierLabel } from "../format";
import { useCommands, type Command as PaletteCommand } from "../commands/registry";
import { TestServerPanel, type ConsoleLine, type InstallState, type ServerTab } from "./TestServerPanel";
import styles from "./ProjectShell.module.css";

interface ProjectShellProps {
  project: ProjectView;
  onProjectChange: (project: ProjectView) => void;
  session: SessionStatus | null;
  sessionOpen: boolean;
  onToggleSession: () => void;
  mcp: McpStatus | null;
  onOpenSettings: (section?: SettingsSection) => void;
  onOpenHelp: (topic?: string) => void;
  onOpenPalette: () => void;
  onClose: () => void;
}

const RUNTIME_LIMIT = 2000;
const CONSOLE_LIMIT = 3000;
const CONSOLE_PARSE_DELAY = 250;
let nextConsoleLineId = 1;

function consoleLine(text: string, kind: ConsoleLine["kind"]): ConsoleLine {
  return { id: nextConsoleLineId++, text, kind };
}

interface DialogState {
  mode: EntryDialogMode;
  locationLabel: string;
  packagePath: string;
  submit: (name: string, kind: GraphKind, parent: string) => Promise<ProjectView>;
  selectCreated?: (graph: GraphSummary) => boolean;
  afterSubmit?: () => void;
}

function describeLocation(pkg: PackageSummary | undefined, location: EntryLocation): string {
  const context = location.context[0].toUpperCase() + location.context.slice(1);
  return [pkg?.folder ?? "", context, ...location.folders].filter(Boolean).join(" / ");
}

function sameFolders(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((segment, index) => segment === right[index]);
}

const NO_PEERS: RemotePeer[] = [];
const DEFINITION_EDITORS: string[] = ["event", "struct", "enum", "dispatcher"];
const SEPARATE_EDITORS: string[] = [...DEFINITION_EDITORS, "interface"];
const PRESENCE_HEARTBEAT = 3000;
const PRESENCE_TIMEOUT = 12000;
const AGENT_CURSOR_MILLISECONDS = 8000;
const AGENT_PEER = -1;
const AGENT_COLOR = "#c084fc";

interface AgentCursor {
  name: string;
  graph: string;
  x: number;
  y: number;
  selection: string[];
}

export function ProjectShell({ project, onProjectChange, session, sessionOpen, onToggleSession, mcp, onOpenSettings, onOpenHelp, onOpenPalette, onClose }: ProjectShellProps) {
  const { notify, reportError } = useToasts();
  const [selectedGraphId, setSelectedGraphId] = useState<string | null>(
    () => project.packages[0]?.graphs.find((graph) => graph.context === "server")?.id ?? project.packages[0]?.graphs[0]?.id ?? null,
  );
  const [catalog, setCatalog] = useState<NodeCatalog | null>(null);
  const [catalogError, setCatalogError] = useState<CommandError | null>(null);
  const [nodeCounts, setNodeCounts] = useState<Record<string, number>>({});
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [build, setBuild] = useState<BuildState | null>(null);
  const [buildOpen, setBuildOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [editorRevision, setEditorRevision] = useState(0);
  const [scopeRevision, setScopeRevision] = useState(0);
  const [remoteChange, setRemoteChange] = useState<RemoteChange | null>(null);
  const remoteRevisionRef = useRef(0);
  const [agentCursor, setAgentCursor] = useState<AgentCursor | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [presences, setPresences] = useState<Map<number, Presence & { seen: number }>>(new Map());
  const lastPresenceRef = useRef<{ graph: string | null; x: number; y: number; selection: string[] } | null>(null);
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null);
  const [sidePanel, setSidePanel] = useState<SidePanel | null>("details");
  const [search, setSearch] = useState<SearchRequest | null>(null);
  const [runtimeOpen, setRuntimeOpen] = useState(false);
  const [runtimeEntries, setRuntimeEntries] = useState<LogEntry[]>([]);
  const [pausedAt, setPausedAt] = useState<BreakpointHit | null>(null);
  const [debuggerError, setDebuggerError] = useState<string | null>(null);
  const [serverStatus, setServerStatus] = useState<TestServerStatus | null>(null);
  const [serverOpen, setServerOpen] = useState(false);
  const [serverTab, setServerTab] = useState<ServerTab>("console");
  const [serverLines, setServerLines] = useState<ConsoleLine[]>([]);
  const [install, setInstall] = useState<InstallState>({ progress: null, line: null, result: null });
  const [playing, setPlaying] = useState(false);
  const flushRef = useRef<(() => Promise<void>) | null>(null);
  const buildingRef = useRef(false);
  const pendingConsoleRef = useRef<string[]>([]);
  const consoleTimerRef = useRef<number | null>(null);

  const [apiRevision, setApiRevision] = useState(0);

  useEffect(() => {
    const subscription = listen("api-updated", () => {
      setApiRevision((value) => value + 1);
      setScopeRevision((value) => value + 1);
    });
    return () => {
      void subscription.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    setCatalogError(null);
    api
      .nodeCatalog(project.apiChannel)
      .then((payload) => {
        if (!cancelled) {
          setCatalog(new NodeCatalog(payload));
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setCatalogError(toCommandError(error));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [project.apiChannel, apiRevision]);

  const selection = useMemo(() => {
    for (const pkg of project.packages) {
      const graph = pkg.graphs.find((candidate) => candidate.id === selectedGraphId);
      if (graph) {
        return { pkg, graph };
      }
    }
    return null;
  }, [project.packages, selectedGraphId]);

  const projectRoot = useMemo(() => normalizePath(project.projectFile).replace(/\/[^/]*$/, ""), [project.projectFile]);
  const sessionLive = sessionState(session) === "live";
  const sharedProject =
    (session?.view.type === "hosting" || session?.view.type === "joined") && normalizePath(session.view.projectFile) === normalizePath(project.projectFile);
  const guestGame = sharedProject && session?.view.type === "joined" ? session.view.game : null;
  const hostingGame = sharedProject && session?.view.type === "hosting";
  const joinGame = useCallback(async () => {
    try {
      const message = await api.joinSessionGame();
      notify({ kind: "success", title: "Joining the game", message });
    } catch (error) {
      reportError(toCommandError(error));
    }
  }, [notify, reportError]);

  const relativeGraph = useCallback(
    (graphPath: string) => {
      const normalized = normalizePath(graphPath);
      return normalized.startsWith(`${projectRoot}/`) ? normalized.slice(projectRoot.length + 1) : normalized;
    },
    [projectRoot],
  );

  const selectedGraphRef = useRef<{ path: string; kind: string } | null>(null);
  selectedGraphRef.current = selection ? { path: normalizePath(selection.graph.path), kind: selection.graph.kind } : null;

  const applyExternalChange = useCallback(
    (relativePaths: string[], undoable: boolean) => {
      const paths = relativePaths.map((path) => `${projectRoot}/${path}`);
      remoteRevisionRef.current += 1;
      setRemoteChange({ revision: remoteRevisionRef.current, paths, undoable });
      const selected = selectedGraphRef.current;
      if (selected && SEPARATE_EDITORS.includes(selected.kind) && paths.some((path) => path.startsWith(`${selected.path}/`))) {
        setEditorRevision((value) => value + 1);
      }
      if (relativePaths.some((path) => !path.includes("/nodes/"))) {
        api
          .openProject(project.projectFile)
          .then((refreshed) => {
            onProjectChange(refreshed);
            setScopeRevision((value) => value + 1);
          })
          .catch(() => undefined);
      }
    },
    [projectRoot, project.projectFile, onProjectChange],
  );

  useEffect(() => {
    if (!sharedProject) {
      return;
    }
    const subscription = listen<SessionFilesMessage>("session-files", ({ payload }) => {
      if (normalizePath(payload.projectRoot) === projectRoot) {
        applyExternalChange(payload.paths, false);
      }
    });
    return () => {
      void subscription.then((unlisten) => unlisten());
    };
  }, [sharedProject, projectRoot, applyExternalChange]);

  useEffect(() => {
    const files = listen<AgentFilesMessage>("agent-files", ({ payload }) => {
      if (normalizePath(payload.projectRoot) !== projectRoot) {
        return;
      }
      applyExternalChange(payload.paths, true);
      if (payload.graph && payload.focus) {
        setAgentCursor({ name: payload.agent, graph: payload.graph, x: payload.focus[0], y: payload.focus[1], selection: payload.nodes });
      }
    });
    const opens = listen<AgentOpenMessage>("agent-open-graph", ({ payload }) => {
      setSelectedGraphId(payload.graphId);
      if (payload.node) {
        setFocusRequest({ graphId: payload.graphId, node: payload.node, token: Date.now() });
      }
    });
    return () => {
      void files.then((unlisten) => unlisten());
      void opens.then((unlisten) => unlisten());
    };
  }, [projectRoot, applyExternalChange]);

  useEffect(() => {
    if (!agentCursor) {
      return;
    }
    const timer = window.setTimeout(() => setAgentCursor(null), AGENT_CURSOR_MILLISECONDS);
    return () => window.clearTimeout(timer);
  }, [agentCursor]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 5000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!sessionLive) {
      setPresences(new Map());
      return;
    }
    const subscription = listen<Presence>("session-presence", ({ payload }) => {
      setPresences((current) => {
        const next = new Map(current);
        if (payload.gone) {
          next.delete(payload.peer);
        } else {
          next.set(payload.peer, { ...payload, seen: Date.now() });
        }
        return next;
      });
    });
    const heartbeat = window.setInterval(() => {
      if (lastPresenceRef.current) {
        void api.updatePresence(lastPresenceRef.current).catch(() => undefined);
      }
      setPresences((current) => {
        const fresh = [...current.values()].filter((presence) => Date.now() - presence.seen < PRESENCE_TIMEOUT);
        return fresh.length === current.size ? current : new Map(fresh.map((presence) => [presence.peer, presence]));
      });
    }, PRESENCE_HEARTBEAT);
    return () => {
      window.clearInterval(heartbeat);
      void subscription.then((unlisten) => unlisten());
    };
  }, [sessionLive]);

  const currentGraph = selection ? relativeGraph(selection.graph.path) : null;
  const peers = useMemo<RemotePeer[]>(
    () =>
      [...presences.values()]
        .filter((presence) => presence.graph !== null && presence.graph === currentGraph)
        .map((presence) => ({
          peer: presence.peer,
          name: presence.name,
          color: peerColor(presence.peer),
          x: presence.x,
          y: presence.y,
          selection: presence.selection,
        })),
    [presences, currentGraph],
  );
  const editorPeers = useMemo<RemotePeer[]>(() => {
    const visible = sharedProject ? peers : NO_PEERS;
    if (!agentCursor || agentCursor.graph !== currentGraph) {
      return visible;
    }
    return [
      ...visible,
      { peer: AGENT_PEER, name: agentCursor.name, color: AGENT_COLOR, x: agentCursor.x, y: agentCursor.y, selection: agentCursor.selection },
    ];
  }, [sharedProject, peers, agentCursor, currentGraph]);

  useEffect(() => {
    void api.reportEditorContext({ projectFile: project.projectFile, graph: selection?.graph.path ?? null }).catch(() => undefined);
  }, [project.projectFile, selection?.graph.path]);

  useEffect(() => () => void api.reportEditorContext(null).catch(() => undefined), []);

  const reportPresence = useCallback(
    (presence: { x: number; y: number; selection: string[] }) => {
      const update = { graph: currentGraph, ...presence };
      lastPresenceRef.current = update;
      void api.updatePresence(update).catch(() => undefined);
    },
    [currentGraph],
  );

  useEffect(() => {
    if (sharedProject && lastPresenceRef.current && lastPresenceRef.current.graph !== currentGraph) {
      reportPresence({ x: lastPresenceRef.current.x, y: lastPresenceRef.current.y, selection: [] });
    }
  }, [sharedProject, currentGraph, reportPresence]);

  const updateNodeCount = useCallback((graphId: string, count: number) => {
    setNodeCounts((current) => (current[graphId] === count ? current : { ...current, [graphId]: count }));
  }, []);

  const registerFlush = useCallback((flush: (() => Promise<void>) | null) => {
    flushRef.current = flush;
  }, []);

  const mutate = useCallback(
    async (action: () => Promise<ProjectView>): Promise<ProjectView> => {
      await flushRef.current?.();
      const updated = await action();
      onProjectChange(updated);
      return updated;
    },
    [onProjectChange],
  );

  const openCollapse = useCallback(
    (pkg: PackageSummary, graph: GraphSummary, nodes: string[]) => {
      const ownerClass = graph.kind === "class" ? graph : pkg.graphs.find((candidate) => candidate.id === graph.owner);
      let call: string | null = null;
      setDialog({
        mode: { type: "collapse", className: ownerClass?.name ?? null, nodeCount: nodes.length },
        locationLabel: ownerClass
          ? describeLocation(pkg, { packagePath: pkg.path, context: ownerClass.context, folders: [...ownerClass.folders, ownerClass.name] })
          : describeLocation(pkg, { packagePath: pkg.path, context: graph.context, folders: graph.folders }),
        packagePath: pkg.path,
        submit: async (name) => {
          const result = await api.collapseToFunction(project.apiChannel, project.projectFile, pkg.path, graph.path, nodes, name);
          call = result.function.call;
          return result.project;
        },
        afterSubmit: () => {
          setEditorRevision((revision) => revision + 1);
          if (call) {
            setFocusRequest({ graphId: graph.id, node: call, token: Date.now() });
          }
        },
      });
    },
    [project.apiChannel, project.projectFile],
  );

  const appendRuntime = useCallback((entries: LogEntry[], replace: boolean) => {
    setRuntimeEntries((current) => (replace ? entries : [...current, ...entries]).slice(-RUNTIME_LIMIT));
  }, []);

  const runtimeMarks = useMemo(() => {
    const marks: Record<string, RuntimeMark> = {};
    for (const entry of runtimeEntries) {
      if (entry.watch) {
        const mark = (marks[entry.watch.node] ??= { errors: 0 });
        mark.watch = entry.watch.values || "ran";
      } else if (entry.severity === "error") {
        const node = entry.locations.find((location) => location.node)?.node;
        if (node) {
          const mark = (marks[node] ??= { errors: 0 });
          mark.errors += 1;
          mark.lastError = entry.text.split("\n")[0];
        }
      }
    }
    return marks;
  }, [runtimeEntries]);

  const markedRuntime = useMemo(() => {
    const node = pausedAt?.node;
    if (!node) {
      return runtimeMarks;
    }
    return { ...runtimeMarks, [node]: { ...(runtimeMarks[node] ?? { errors: 0 }), paused: true } };
  }, [runtimeMarks, pausedAt]);

  const revealHit = useCallback((hit: BreakpointHit) => {
    if (hit.graph && hit.node) {
      setSelectedGraphId(hit.graph);
      setFocusRequest({ graphId: hit.graph, node: hit.node, token: Date.now() });
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    void api.debuggerStatus().then(
      (status) => {
        if (disposed) {
          return;
        }
        setDebuggerError(status.error);
        setPausedAt(status.paused);
      },
      () => undefined,
    );
    const subscription = listen<DebuggerMessage>("debugger", ({ payload }) => {
      if (payload.type === "paused") {
        setPausedAt(payload.hit);
        revealHit(payload.hit);
      } else if (payload.type === "resumed") {
        setPausedAt(null);
      } else {
        const { hit } = payload;
        const values = hit.values.map(([name, value]) => `${name} = ${value}`).join(", ");
        appendRuntime(
          [
            {
              severity: "watch",
              text: `Breakpoint reached on the ${hit.side}: ${values}`,
              locations: [],
              watch: hit.graph && hit.node ? { graph: hit.graph, node: hit.node, values: values || "reached" } : undefined,
            },
          ],
          false,
        );
      }
    });
    return () => {
      disposed = true;
      void subscription.then((unsubscribe) => unsubscribe());
    };
  }, [appendRuntime, revealHit]);

  const pausedLocation = (hit: BreakpointHit) => {
    const graph = project.packages.flatMap((pkg) => pkg.graphs).find((candidate) => candidate.id === hit.graph);
    return graph ? `${graph.name}, ${graph.context} graph. Click to show the node.` : hit.location;
  };

  const resumeRun = useCallback((mode: ResumeMode) => {
    setPausedAt(null);
    void api.debuggerResume(mode);
  }, []);

  const describeFrame = useCallback(
    (frame: StackFrame) => {
      const graph = project.packages.flatMap((pkg) => pkg.graphs).find((candidate) => candidate.id === frame.graph);
      return graph ? graph.name : frame.location;
    },
    [project.packages],
  );

  const refreshServer = useCallback(() => {
    api.testServerStatus(project.projectFile).then(setServerStatus, () => undefined);
  }, [project.projectFile]);

  useEffect(() => {
    refreshServer();
  }, [refreshServer, project]);

  const pushConsole = useCallback((lines: ConsoleLine[]) => {
    setServerLines((current) => [...current, ...lines].slice(-CONSOLE_LIMIT));
  }, []);

  const parseConsoleSoon = useCallback(
    (line: string) => {
      pendingConsoleRef.current.push(line);
      if (consoleTimerRef.current !== null) {
        return;
      }
      consoleTimerRef.current = window.setTimeout(() => {
        consoleTimerRef.current = null;
        const text = pendingConsoleRef.current.join("\n");
        pendingConsoleRef.current = [];
        api.parseRuntimeLog(project.projectFile, text).then(
          (entries) => appendRuntime(entries, false),
          () => undefined,
        );
      }, CONSOLE_PARSE_DELAY);
    },
    [appendRuntime, project.projectFile],
  );

  useEffect(() => {
    const subscription = listen<TestServerMessage>("test-server", ({ payload }) => {
      if (payload.type === "output") {
        const isCommand = payload.line.startsWith("> ");
        pushConsole([consoleLine(payload.line, isCommand ? "command" : "output")]);
        if (!isCommand) {
          parseConsoleSoon(payload.line);
        }
      } else if (payload.type === "state") {
        if (!payload.running) {
          pushConsole([
            consoleLine(
              payload.exitCode === null || payload.exitCode === 0 ? "Server stopped." : `Server stopped with exit code ${payload.exitCode}.`,
              "notice",
            ),
          ]);
        }
        refreshServer();
      } else if (payload.type === "install") {
        setInstall((current) => ({ progress: payload.progress ?? current.progress, line: payload.line, result: null }));
      } else {
        setInstall({ progress: null, line: null, result: { success: payload.success, message: payload.message } });
        refreshServer();
        if (payload.success) {
          notify({ kind: "success", title: "Test server installed", message: "Press Play to build your packages and start it." });
        } else {
          notify({ kind: "error", title: "Installation failed", message: payload.message });
        }
      }
    });
    return () => {
      void subscription.then((unsubscribe) => unsubscribe());
      if (consoleTimerRef.current !== null) {
        window.clearTimeout(consoleTimerRef.current);
        consoleTimerRef.current = null;
      }
    };
  }, [notify, parseConsoleSoon, pushConsole, refreshServer]);

  const installServer = useCallback(async () => {
    setInstall({ progress: null, line: null, result: null });
    try {
      await api.installTestServer(project.projectFile);
      refreshServer();
    } catch (error) {
      reportError(toCommandError(error));
    }
  }, [project.projectFile, refreshServer, reportError]);

  const playServer = useCallback(
    async (restart: boolean) => {
      if (playing || !catalog) {
        return;
      }
      if (serverStatus && !serverStatus.installed) {
        setServerTab("setup");
        setServerOpen(true);
        return;
      }
      setPlaying(true);
      try {
        await flushRef.current?.();
        const result = await api.playTestServer(project.apiChannel, project.projectFile, restart);
        setBuild({ status: "done", report: result.report });
        if (result.action === "build-failed") {
          setBuildOpen(true);
          return;
        }
        const notices: Record<typeof result.action, string> = {
          started: `Starting the server. Join with Quick Connect and ${result.address}.`,
          restarted: `Restarting the server. Join again with Quick Connect and ${result.address}.`,
          reloaded: "Built and reloaded your packages.",
        };
        pushConsole([consoleLine(notices[result.action], "notice")]);
        if (result.action !== "reloaded") {
          notify({ kind: "success", title: result.action === "started" ? "Test server starting" : "Test server restarting", message: notices[result.action] });
        }
        refreshServer();
      } catch (error) {
        const commandError = toCommandError(error);
        reportError(commandError);
        pushConsole([consoleLine(`${commandError.title}: ${commandError.message}`, "notice")]);
      } finally {
        setPlaying(false);
      }
    },
    [catalog, notify, playing, project.apiChannel, project.projectFile, pushConsole, refreshServer, reportError, serverStatus],
  );

  const stopServer = useCallback(async () => {
    try {
      await api.stopTestServer();
    } catch (error) {
      reportError(toCommandError(error));
    }
    refreshServer();
  }, [refreshServer, reportError]);

  const runtimeErrors = runtimeEntries.filter((entry) => entry.severity === "error").length;

  const openRuntimeLocation = useCallback((graphId: string, nodeId: string) => {
    setSelectedGraphId(graphId);
    setFocusRequest({ graphId, node: nodeId, token: Date.now() });
  }, []);

  const openSearch = useCallback(async (request: SearchRequest) => {
    await flushRef.current?.();
    setSearch(request);
  }, []);

  const findUsages = useCallback((target: string, label: string) => void openSearch({ mode: "usages", target, label }), [openSearch]);

  const openHit = useCallback(
    (hit: SearchHit) => {
      setSearch(null);
      setSelectedGraphId(hit.graphId);
      if (hit.node) {
        setFocusRequest({ graphId: hit.graphId, node: hit.node, token: Date.now() });
      } else if (hit.kind === "variable" || hit.kind === "type") {
        setSidePanel("details");
      }
    },
    [],
  );

  const runBuild = useCallback(async () => {
    if (buildingRef.current) {
      return;
    }
    buildingRef.current = true;
    setBuild((current) => ({ status: "building", previous: current?.status === "done" ? current.report : null }));
    setBuildOpen(true);
    try {
      await flushRef.current?.();
      const report = await api.buildProject(project.apiChannel, project.projectFile);
      setBuild({ status: "done", report });
    } catch (error) {
      setBuild({ status: "failed", error: toCommandError(error) });
    } finally {
      buildingRef.current = false;
    }
  }, [project.apiChannel, project.projectFile]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.altKey && !event.ctrlKey && !event.metaKey && event.code === "KeyP") {
        event.preventDefault();
        if (guestGame) {
          if (guestGame.running) {
            void joinGame();
          }
        } else {
          void playServer(false);
        }
      } else if ((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "b") {
        event.preventDefault();
        void runBuild();
      } else if ((event.ctrlKey || event.metaKey) && event.shiftKey && !event.altKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        void openSearch({ mode: "text", query: "" });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [runBuild, openSearch, playServer, guestGame, joinGame]);

  const closeBuild = useCallback(() => setBuildOpen(false), []);

  const openProblem = useCallback(
    (problem: BuildProblem) => {
      const graphId = problem.graph?.id;
      if (!graphId || !project.packages.some((pkg) => pkg.graphs.some((graph) => graph.id === graphId))) {
        return;
      }
      setSelectedGraphId(graphId);
      if (problem.node) {
        setFocusRequest({ graphId, node: problem.node, token: Date.now() });
      }
    },
    [project.packages],
  );

  const packageOf = (path: string) => project.packages.find((pkg) => pkg.path === path);

  const startCreate = (location: EntryLocation, request: CreateRequest) => {
    const locationLabel = describeLocation(packageOf(location.packagePath), location);
    if (request === "folder") {
      setDialog({
        mode: { type: "create-folder" },
        locationLabel,
        packagePath: location.packagePath,
        submit: (name) => api.createFolder(project.projectFile, location, name),
      });
    } else {
      setDialog({
        mode: { type: "create-graph", kind: request },
        locationLabel,
        packagePath: location.packagePath,
        submit: (name, kind, parent) =>
          api.createGraph(project.apiChannel, project.projectFile, location, name, kind, kind === "class" ? parent : undefined),
        selectCreated: (graph) => !graph.owner && graph.context === location.context && sameFolders(graph.folders, location.folders),
      });
    }
  };

  const startCreateMethod = (classGraph: GraphSummary) => {
    const pkg = project.packages.find((candidate) => candidate.graphs.some((graph) => graph.id === classGraph.id));
    if (!pkg) {
      return;
    }
    setDialog({
      mode: { type: "create-method", className: classGraph.name },
      locationLabel: describeLocation(pkg, { packagePath: pkg.path, context: classGraph.context, folders: [...classGraph.folders, classGraph.name] }),
      packagePath: pkg.path,
      submit: (name) => api.createMethod(project.projectFile, classGraph.path, name),
      selectCreated: (graph) => graph.owner === classGraph.id,
    });
  };

  const startCreateDispatcher = (classGraph: GraphSummary) => {
    const pkg = project.packages.find((candidate) => candidate.graphs.some((graph) => graph.id === classGraph.id));
    if (!pkg) {
      return;
    }
    setDialog({
      mode: { type: "create-dispatcher", className: classGraph.name },
      locationLabel: describeLocation(pkg, { packagePath: pkg.path, context: classGraph.context, folders: [...classGraph.folders, classGraph.name] }),
      packagePath: pkg.path,
      submit: (name) => api.createDispatcher(project.projectFile, classGraph.path, name),
      selectCreated: (graph) => graph.owner === classGraph.id && graph.kind === "dispatcher",
    });
  };

  const startRename = (entry: TreeEntry) => {
    const item = entry.type === "graph" ? entry.graph : entry.folder;
    setDialog({
      mode: {
        type: "rename",
        isFolder: entry.type === "folder",
        isClass: entry.type === "graph" && ["class", "struct", "enum", "interface"].includes(entry.graph.kind),
        currentName: item.name,
      },
      locationLabel: describeLocation(entry.pkg, { packagePath: entry.pkg.path, context: item.context, folders: item.folders }),
      packagePath: entry.pkg.path,
      submit: async (name) => {
        const updated = await api.renameEntry(project.projectFile, item.path, name);
        if (entry.type === "graph" && ["class", "struct", "enum", "interface"].includes(entry.graph.kind)) {
          setEditorRevision((revision) => revision + 1);
        }
        return updated;
      },
    });
  };

  const deleteEntry = async (entry: TreeEntry) => {
    const isFolder = entry.type === "folder";
    const item = isFolder ? entry.folder : entry.graph;
    const contained = isFolder
      ? entry.pkg.graphs.filter(
          (graph) =>
            graph.context === entry.folder.context &&
            [...entry.folder.folders, entry.folder.name].every((segment, index) => graph.folders[index] === segment),
        ).length
      : 0;
    const message = isFolder
      ? `Delete the folder "${item.name}"${contained > 0 ? ` and the ${contained} ${contained === 1 ? "graph" : "graphs"} inside it` : ""}?`
      : entry.graph.kind === "event"
        ? `Delete the network event "${item.name}"? Nodes that send or handle it stop working.`
        : ["struct", "enum", "interface"].includes(entry.graph.kind)
        ? `Delete the ${entry.graph.kind} "${item.name}"? Variables, pins, classes and nodes that use it stop working.`
        : entry.graph.kind === "dispatcher"
        ? `Delete the event dispatcher "${item.name}"? Nodes that bind or call it stop working.`
        : `Delete the ${entry.graph.kind === "function" ? "function" : entry.graph.kind === "macro" ? "macro" : entry.graph.kind === "class" ? "class" : "graph"} "${item.name}" with ${entry.graph.nodeCount} ${entry.graph.nodeCount === 1 ? "node" : "nodes"}?`;
    const confirmed = await ask(`${message}\n\nThe files are removed from disk. Changes that are not committed to version control cannot be restored.`, {
      title: "Delete",
      kind: "warning",
      okLabel: "Delete",
      cancelLabel: "Cancel",
    });
    if (!confirmed) {
      return;
    }
    try {
      await mutate(() => api.deleteEntry(project.projectFile, item.path));
    } catch (error) {
      reportError(toCommandError(error));
    }
  };

  const moveEntry = async (entry: TreeEntry, target: EntryLocation) => {
    const item = entry.type === "graph" ? entry.graph : entry.folder;
    try {
      await mutate(() => api.moveEntry(project.projectFile, item.path, target));
    } catch (error) {
      reportError(toCommandError(error));
    }
  };

  function renderCanvas() {
    if (catalogError) {
      return (
        <div className={styles.placeholder}>
          <CircleAlert size={28} />
          <h2>{catalogError.title}</h2>
          <p>{catalogError.message}</p>
        </div>
      );
    }
    if (!catalog) {
      return (
        <div className={styles.placeholder}>
          <LoaderCircle size={24} className={styles.spin} />
          <h2>Loading the nanos world API</h2>
        </div>
      );
    }
    if (!selection) {
      return (
        <div className={styles.placeholder}>
          <Workflow size={28} />
          <h2>No graph selected</h2>
          <p>Pick a graph on the left, or right-click a section or folder to create an event graph, a function or a folder.</p>
        </div>
      );
    }
    if (selection.graph.kind === "interface") {
      return (
        <InterfaceEditor
          key={`${selection.graph.path}:${editorRevision}`}
          graph={selection.graph}
          packagePath={selection.pkg.path}
          apiChannel={project.apiChannel}
          catalog={catalog}
          onRegisterFlush={registerFlush}
        />
      );
    }
    if (DEFINITION_EDITORS.includes(selection.graph.kind)) {
      return (
        <DefinitionEditor
          key={`${selection.graph.path}:${editorRevision}`}
          graph={selection.graph}
          packagePath={selection.pkg.path}
          apiChannel={project.apiChannel}
          catalog={catalog}
          onRegisterFlush={registerFlush}
        />
      );
    }
    return (
      <GraphEditor
        key={`${selection.graph.path}:${editorRevision}`}
        graph={selection.graph}
        packageFolder={selection.pkg.folder}
        packagePath={selection.pkg.path}
        apiChannel={project.apiChannel}
        catalog={catalog}
        onNodeCountChange={updateNodeCount}
        onRegisterFlush={registerFlush}
        projectFile={project.projectFile}
        onProjectChange={onProjectChange}
        focusRequest={focusRequest}
        sidePanel={sidePanel}
        onSidePanelChange={setSidePanel}
        onCollapseSelection={(nodes) => openCollapse(selection.pkg, selection.graph, nodes)}
        onFindUsages={findUsages}
        runtime={markedRuntime}
        scopeRevision={scopeRevision}
        remoteChange={remoteChange}
        peers={editorPeers}
        onPresence={sharedProject ? reportPresence : null}
      />
    );
  }

  const paletteCommands: PaletteCommand[] = [
    { id: "project.build", title: "Build the project", group: "Run", shortcut: `${modifierLabel} B`, keywords: "compile lua", run: () => void runBuild() },
    guestGame
      ? { id: "project.join", title: "Join the host's game", group: "Run", shortcut: "Alt P", keywords: "play connect", run: () => void joinGame() }
      : {
          id: "project.play",
          title: serverStatus?.runningHere ? "Reload the test server" : "Play on the test server",
          group: "Run",
          shortcut: "Alt P",
          keywords: "start run server test",
          run: () => void playServer(false),
        },
    ...(serverStatus?.runningHere && !guestGame
      ? [
          { id: "project.restart", title: "Restart the test server", group: "Run" as const, keywords: "server", run: () => void playServer(true) },
          { id: "project.stop", title: "Stop the test server", group: "Run" as const, keywords: "server", run: () => void stopServer() },
        ]
      : []),
    { id: "project.search", title: "Search the whole project", group: "Project", shortcut: `${modifierLabel} Shift F`, keywords: "find text", run: () => void openSearch({ mode: "text", query: "" }) },
    { id: "project.settings", title: "Project settings", group: "Project", keywords: "output requirements custom settings config", run: () => setSettingsOpen(true) },
    { id: "project.reveal", title: "Show the project in its folder", group: "Project", keywords: "explorer files reveal", run: () => void revealItemInDir(project.projectFile) },
    { id: "project.close", title: "Close the project", group: "Project", keywords: "back start screen", run: onClose },
    { id: "panel.runtime", title: runtimeOpen ? "Hide the runtime log" : "Show the runtime log", group: "Panels", keywords: "errors log", run: () => setRuntimeOpen((value) => !value) },
    { id: "panel.server", title: serverOpen ? "Hide the test server panel" : "Show the test server panel", group: "Panels", keywords: "console install", run: () => setServerOpen((value) => !value) },
    ...(build ? [{ id: "panel.build", title: buildOpen ? "Hide the build result" : "Show the build result", group: "Panels" as const, keywords: "errors problems", run: () => setBuildOpen((value) => !value) }] : []),
    { id: "panel.agents", title: "AI agent settings", group: "Panels", keywords: "mcp claude", run: () => onOpenSettings("agents") },
    ...project.packages.flatMap((pkg) =>
      pkg.graphs.map((graph) => ({
        id: `graph.${graph.id}`,
        title: graph.name,
        group: "Graphs" as const,
        detail: [pkg.title, graph.context, ...graph.folders].join(" / "),
        keywords: `${graph.kind} graph open`,
        run: () => setSelectedGraphId(graph.id),
      })),
    ),
  ];
  useCommands("project", paletteCommands);

  return (
    <div className={styles.shell}>
      <header className={styles.titlebar}>
        <Button variant="ghost" size="small" icon={<ChevronLeft size={16} />} onClick={onClose} title="Back to the start screen" />
        <span className={styles.projectName}>{project.name}</span>
        <span className={styles.channel}>API {project.apiChannel}</span>
        {catalog && <span className={styles.channel}>{catalog.size} nodes</span>}
        <span className={styles.spacer} />
        <button
          type="button"
          className={styles.runtimeButton}
          data-active={runtimeOpen}
          aria-label="Runtime"
          onClick={() => setRuntimeOpen((value) => !value)}
          title="Follow the nanos world log, see runtime errors on their nodes and watched values"
        >
          <Activity size={14} />
          <span className={styles.buttonLabel}>Runtime</span>
          {runtimeErrors > 0 && <span className={styles.runtimeCount}>{runtimeErrors}</span>}
        </button>
        <button
          type="button"
          className={styles.runtimeButton}
          data-active={serverOpen}
          aria-label="Server"
          onClick={() => setServerOpen((value) => !value)}
          title="Install, configure and watch the local test server"
        >
          <Server size={14} />
          <span className={styles.buttonLabel}>Server</span>
          <span className={styles.serverDot} data-state={serverStatus?.runningHere ? "running" : serverStatus?.installing ? "busy" : "idle"} />
        </button>
        <button
          type="button"
          className={styles.runtimeButton}
          data-active={sessionOpen}
          aria-label="Session"
          onClick={onToggleSession}
          title="Host or join a session to play together"
        >
          <Users size={14} />
          <span className={styles.buttonLabel}>Session</span>
          {session?.view.type === "hosting" && session.view.requests.length > 0 ? (
            <span className={styles.runtimeCount}>{session.view.requests.length}</span>
          ) : (
            <span className={styles.serverDot} data-state={{ idle: "idle", busy: "busy", live: "running" }[sessionState(session)]} />
          )}
        </button>
        <button
          type="button"
          className={styles.runtimeButton}
          aria-label="Agents"
          onClick={() => onOpenSettings("agents")}
          title={mcp?.settings.enabled ? `AI agents can control NanosFlow through ${mcp.url}` : "Let AI agents like Claude Code control NanosFlow"}
        >
          <Bot size={14} />
          <span className={styles.buttonLabel}>Agents</span>
          <span
            className={styles.serverDot}
            data-state={mcp?.error ? "busy" : agentActive(mcp, now) ? "running" : "idle"}
          />
        </button>
        <button
          type="button"
          className={styles.runtimeButton}
          aria-label="Search"
          onClick={() => void openSearch({ mode: "text", query: "" })}
          title={`Search the whole project (${modifierLabel}+Shift+F)`}
        >
          <Search size={14} />
          <span className={styles.buttonLabel}>Search</span>
        </button>
        <span className={styles.divider} />
        <Button variant="ghost" size="small" icon={<Command size={15} />} onClick={onOpenPalette} title={`Command palette (${modifierLabel}+Shift+P)`} aria-label="Command palette" />
        <Button variant="ghost" size="small" icon={<FolderSearch size={15} />} onClick={() => revealItemInDir(project.projectFile)} title="Show the project in its folder" aria-label="Show in folder" />
        <Button
          variant="ghost"
          size="small"
          icon={<Settings size={15} />}
          onClick={() => setSettingsOpen(true)}
          title="Project settings: build output, requirements and custom settings"
          aria-label="Project settings"
        />
        <Button variant="ghost" size="small" icon={<CircleHelp size={15} />} onClick={() => onOpenHelp()} title="Help (F1)" aria-label="Help" />
        {build && build.status !== "building" && (
          <button
            type="button"
            className={styles.buildStatus}
            data-tone={
              build.status === "failed" || build.report.errorCount > 0 ? "error" : build.report.warningCount > 0 ? "warning" : "success"
            }
            onClick={() => setBuildOpen((open) => !open)}
            title="Show the last build result"
            aria-expanded={buildOpen}
          >
            {build.status === "failed" || build.report.errorCount > 0 ? (
              <>
                <CircleAlert size={14} />
                {build.status === "failed" ? "Failed" : `${build.report.errorCount} ${build.report.errorCount === 1 ? "error" : "errors"}`}
              </>
            ) : build.report.warningCount > 0 ? (
              <>
                <TriangleAlert size={14} />
                {build.report.warningCount} {build.report.warningCount === 1 ? "warning" : "warnings"}
              </>
            ) : (
              <>
                <CircleCheck size={14} />
                Built
              </>
            )}
          </button>
        )}
        <Button
          variant="primary"
          size="small"
          icon={<Hammer size={14} />}
          busy={build?.status === "building"}
          aria-label="Build"
          onClick={() => void runBuild()}
          disabled={!catalog}
          title={`Compile every package to Lua (${modifierLabel}+B)`}
        >
          <span className={styles.buttonLabel}>Build</span>
        </Button>
        {guestGame ? (
          <span className={styles.playGroup}>
            <button
              type="button"
              className={styles.playButton}
              disabled={!guestGame.running}
              onClick={() => void joinGame()}
              title={
                guestGame.running
                  ? "Start nanos world and connect to the test server of the host through the session (Alt+P)"
                  : "Available as soon as the host starts the test server with Play"
              }
            >
              <Play size={14} />
              Join game
            </button>
          </span>
        ) : (
        <span className={styles.playGroup}>
          <button
            type="button"
            className={styles.playButton}
            disabled={!catalog || playing || Boolean(serverStatus?.installing)}
            onClick={() => void playServer(false)}
            title={
              hostingGame
                ? serverStatus?.runningHere
                  ? "Build and reload your packages, your guests play on this server too (Alt+P)"
                  : "Build and start the test server, your guests can then join the game (Alt+P)"
                : serverStatus?.runningHere
                  ? "Build and reload your packages on the test server (Alt+P)"
                  : "Build and start the local test server (Alt+P)"
            }
          >
            {playing ? <LoaderCircle size={14} className={styles.spin} /> : serverStatus?.runningHere ? <RotateCw size={14} /> : <Play size={14} />}
            {serverStatus?.runningHere ? "Reload" : "Play"}
          </button>
          {serverStatus?.runningHere && (
            <button type="button" className={styles.stopButton} onClick={() => void stopServer()} title="Stop the test server">
              <Square size={12} />
            </button>
          )}
        </span>
        )}
      </header>

      {pausedAt && (
        <DebugBar
          hit={pausedAt}
          location={pausedLocation(pausedAt)}
          onResume={resumeRun}
          onReveal={() => revealHit(pausedAt)}
          describeFrame={describeFrame}
          onRevealFrame={(frame) => {
            if (frame.graph && frame.node) {
              openRuntimeLocation(frame.graph, frame.node);
            }
          }}
        />
      )}

      <RuntimePanel
        project={project}
        debuggerError={debuggerError}
        open={runtimeOpen}
        entries={runtimeEntries}
        onEntries={appendRuntime}
        onOpenLocation={openRuntimeLocation}
        onClose={() => setRuntimeOpen(false)}
      />

      {serverOpen && (
        <TestServerPanel
          project={project}
          status={serverStatus}
          lines={serverLines}
          install={install}
          tab={serverTab}
          playing={playing}
          onTabChange={setServerTab}
          onStatusChange={setServerStatus}
          onInstall={() => void installServer()}
          onPlay={(restart) => void playServer(restart)}
          onStop={() => void stopServer()}
          onClearConsole={() => setServerLines([])}
          onClose={() => setServerOpen(false)}
        />
      )}

      {search && (
        <SearchPanel project={project} request={search} onRequestChange={setSearch} onSelect={openHit} onClose={() => setSearch(null)} />
      )}

      {settingsOpen && (
        <ProjectSettings
          project={project}
          onClose={() => setSettingsOpen(false)}
          onProjectChange={(updated) => {
            onProjectChange(updated);
            setScopeRevision((revision) => revision + 1);
          }}
          onOpenHelp={(topic) => {
            setSettingsOpen(false);
            onOpenHelp(topic);
          }}
        />
      )}

      {build && buildOpen && (
        <BuildPanel
          state={build}
          onClose={closeBuild}
          onRebuild={() => void runBuild()}
          onSelectProblem={openProblem}
        />
      )}

      <div className={styles.main}>
        <aside className={styles.sidebar}>
          <ProjectTree
            project={project}
            selectedGraphId={selectedGraphId}
            nodeCounts={nodeCounts}
            onSelectGraph={(graph) => setSelectedGraphId(graph.id)}
            onCreate={startCreate}
            onCreateMethod={startCreateMethod}
            onCreateDispatcher={startCreateDispatcher}
            onRename={startRename}
            onDelete={(entry) => void deleteEntry(entry)}
            onMove={(entry, target) => void moveEntry(entry, target)}
            onFindUsages={(graph) => findUsages(graph.id, graph.name)}
          />
        </aside>

        <main className={styles.canvas}>{renderCanvas()}</main>
      </div>

      <footer className={styles.statusbar}>
        <code>{selection?.graph.path ?? project.projectFile}</code>
        {catalog && <span className={styles.revision}>API revision {catalog.revision}</span>}
      </footer>

      {dialog && (
        <EntryDialog
          mode={dialog.mode}
          locationLabel={dialog.locationLabel}
          parentOptions={{
            api: catalog?.apiClassNames().sort() ?? ["Character"],
            project:
              packageOf(dialog.packagePath)
                ?.graphs.filter((graph) => graph.kind === "class")
                .map((graph) => graph.name)
                .sort() ?? [],
          }}
          onCancel={() => setDialog(null)}
          onSubmit={async (name, kind, parent) => {
            const before = new Set(project.packages.flatMap((pkg) => pkg.graphs.map((graph) => graph.id)));
            const updated = await mutate(() => dialog.submit(name, kind, parent));
            setDialog(null);
            dialog.afterSubmit?.();
            const isCreated = dialog.selectCreated;
            if (isCreated) {
              const created = updated.packages
                .find((pkg) => pkg.path === dialog.packagePath)
                ?.graphs.find((graph) => !before.has(graph.id) && isCreated(graph));
              if (created) {
                setSelectedGraphId(created.id);
              }
            }
          }}
        />
      )}
    </div>
  );
}
