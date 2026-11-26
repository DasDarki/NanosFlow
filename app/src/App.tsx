import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { FileWarning, FolderInput } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, OPEN_PROJECT_EVENT, PROJECT_EXTENSION, toCommandError } from "./api";
import styles from "./App.module.css";
import { useToasts } from "./components/Toasts";
import { ProjectShell } from "./screens/ProjectShell";
import { Welcome } from "./screens/Welcome";
import { SessionPanel } from "./session/SessionPanel";
import { useSession } from "./session/useSession";
import { useUpdateCheck } from "./useUpdateCheck";
import type { AppInfo, NewProjectOptions, ProjectView, RecentProject } from "./types";
import { NewProjectWizard } from "./wizard/NewProjectWizard";

type DropState = { valid: boolean } | null;

function isProjectFile(path: string): boolean {
  return path.toLowerCase().endsWith(`.${PROJECT_EXTENSION}`);
}

export function App() {
  const { reportError } = useToasts();
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [recentProjects, setRecentProjects] = useState<RecentProject[]>([]);
  const [project, setProject] = useState<ProjectView | null>(null);
  const [wizardOptions, setWizardOptions] = useState<NewProjectOptions | null>(null);
  const [dropState, setDropState] = useState<DropState>(null);
  const openingRef = useRef(false);
  const session = useSession();
  useUpdateCheck();
  const [sessionOpen, setSessionOpen] = useState(false);
  const toggleSession = useCallback(() => setSessionOpen((open) => !open), []);

  const refreshRecent = useCallback(async () => {
    setRecentProjects(await api.listRecentProjects().catch(() => []));
  }, []);

  const openProject = useCallback(
    async (path: string) => {
      if (openingRef.current) {
        return;
      }
      openingRef.current = true;
      try {
        const opened = await api.openProject(path);
        setWizardOptions(null);
        setProject(opened);
      } catch (error) {
        reportError(toCommandError(error));
      } finally {
        openingRef.current = false;
        void refreshRecent();
      }
    },
    [refreshRecent, reportError],
  );

  const sessionProject = session.status?.view.type === "joined" ? session.status.view.projectFile : null;
  const openedSessionProjectRef = useRef<string | null>(null);
  useEffect(() => {
    if (!sessionProject || openedSessionProjectRef.current === sessionProject) {
      return;
    }
    openedSessionProjectRef.current = sessionProject;
    void openProject(sessionProject);
  }, [sessionProject, openProject]);

  const browseProject = useCallback(async () => {
    const selected = await open({
      title: "Open NanosFlow project",
      multiple: false,
      filters: [{ name: "NanosFlow Project", extensions: [PROJECT_EXTENSION] }],
    });
    if (typeof selected === "string") {
      await openProject(selected);
    }
  }, [openProject]);

  const startNewProject = useCallback(async () => {
    try {
      setWizardOptions(await api.defaultProjectOptions());
    } catch (error) {
      reportError(toCommandError(error));
    }
  }, [reportError]);

  const removeRecent = useCallback(
    async (path: string) => {
      await api.removeRecentProject(path).catch((error) => reportError(toCommandError(error)));
      void refreshRecent();
    },
    [refreshRecent, reportError],
  );

  useEffect(() => {
    void api.appInfo().then(setAppInfo);
    void refreshRecent();
    void api.takeLaunchProject().then((path) => {
      if (path) {
        void openProject(path);
      }
    });
  }, [refreshRecent, openProject]);

  useEffect(() => {
    const unlisten = listen<string>(OPEN_PROJECT_EVENT, (event) => void openProject(event.payload));
    return () => void unlisten.then((dispose) => dispose());
  }, [openProject]);

  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "enter") {
        setDropState({ valid: payload.paths.some(isProjectFile) });
      } else if (payload.type === "leave") {
        setDropState(null);
      } else if (payload.type === "drop") {
        setDropState(null);
        const projectFile = payload.paths.find(isProjectFile);
        if (projectFile) {
          void openProject(projectFile);
        } else {
          reportError({
            title: "That is not a NanosFlow project",
            message: `Drop a .${PROJECT_EXTENSION} file to open a project.`,
            issues: [],
          });
        }
      }
    });
    return () => void unlisten.then((dispose) => dispose());
  }, [openProject, reportError]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (wizardOptions || !(event.ctrlKey || event.metaKey)) {
        return;
      }
      const key = event.key.toLowerCase();
      if (key === "n") {
        event.preventDefault();
        void startNewProject();
      } else if (key === "o") {
        event.preventDefault();
        void browseProject();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [wizardOptions, startNewProject, browseProject]);

  return (
    <>
      {project ? (
        <ProjectShell
          project={project}
          onProjectChange={setProject}
          session={session.status}
          sessionOpen={sessionOpen}
          onToggleSession={toggleSession}
          onClose={() => {
            setProject(null);
            void refreshRecent();
          }}
        />
      ) : (
        <Welcome
          appInfo={appInfo}
          recentProjects={recentProjects}
          onNewProject={startNewProject}
          onBrowseProject={browseProject}
          onOpenProject={openProject}
          onRemoveRecent={removeRecent}
          session={session.status}
          onOpenSession={() => setSessionOpen(true)}
        />
      )}

      {sessionOpen && <SessionPanel session={session} projectFile={project?.projectFile ?? null} onClose={() => setSessionOpen(false)} />}

      {wizardOptions && (
        <NewProjectWizard
          initialOptions={wizardOptions}
          onCancel={() => setWizardOptions(null)}
          onCreated={(created) => {
            setWizardOptions(null);
            setProject(created);
            void refreshRecent();
          }}
        />
      )}

      {dropState && (
        <div className={styles.dropOverlay} data-valid={dropState.valid}>
          <div className={styles.dropContent}>
            {dropState.valid ? <FolderInput size={32} /> : <FileWarning size={32} />}
            {dropState.valid ? "Drop to open project" : "Not a NanosFlow project"}
            <span>{dropState.valid ? "NanosFlow will open the .nfproj file" : `Only .${PROJECT_EXTENSION} files can be opened`}</span>
          </div>
        </div>
      )}
    </>
  );
}
