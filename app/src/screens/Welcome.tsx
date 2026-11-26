import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { CircleHelp, FolderOpen, FolderSearch, Plus, Search, Settings, Sparkles, Users, X } from "lucide-react";
import { useMemo, useState } from "react";
import { Button } from "../components/Button";
import { TextInput } from "../components/Field";
import { formatRelativeTime, modifierLabel } from "../format";
import { sessionState } from "../session/SessionPanel";
import type { AppInfo, RecentProject, SessionStatus } from "../types";
import styles from "./Welcome.module.css";

interface WelcomeProps {
  appInfo: AppInfo | null;
  recentProjects: RecentProject[];
  onNewProject: () => void;
  onBrowseProject: () => void;
  onOpenProject: (path: string) => void;
  onRemoveRecent: (path: string) => void;
  session: SessionStatus | null;
  onOpenSession: () => void;
  onOpenSettings: () => void;
  onOpenHelp: () => void;
}

export function Welcome({
  appInfo,
  recentProjects,
  onNewProject,
  onBrowseProject,
  onOpenProject,
  onRemoveRecent,
  session,
  onOpenSession,
  onOpenSettings,
  onOpenHelp,
}: WelcomeProps) {
  const [filter, setFilter] = useState("");

  const visibleProjects = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) {
      return recentProjects;
    }
    return recentProjects.filter(
      (project) => project.name.toLowerCase().includes(needle) || project.path.toLowerCase().includes(needle),
    );
  }, [filter, recentProjects]);

  return (
    <div className={styles.screen}>
      <aside className={styles.hero}>
        <img className={styles.logo} src="/logo.webp" alt="" draggable={false} />
        <div className={styles.brand}>
          <h1 className={styles.name}>NanosFlow</h1>
          <p className={styles.tagline}>Visual scripting for nanos world. Build with nodes, ship clean Lua.</p>
        </div>
        <div className={styles.actions}>
          <Button variant="primary" size="large" icon={<Plus size={18} />} onClick={onNewProject}>
            New Project
            <span className={styles.shortcut}>{modifierLabel}+N</span>
          </Button>
          <Button size="large" icon={<FolderOpen size={18} />} onClick={onBrowseProject}>
            Open Project
            <span className={styles.shortcut}>{modifierLabel}+O</span>
          </Button>
          <Button variant="ghost" size="large" icon={<Users size={18} />} onClick={onOpenSession}>
            {sessionState(session) === "idle" ? "Join a session" : "Current session"}
            {sessionState(session) !== "idle" && <span className={styles.sessionDot} data-state={sessionState(session)} />}
          </Button>
          <p className={styles.dropHint}>or drop a .nfproj file anywhere on this window</p>
        </div>
        <footer className={styles.footer}>
          <div className={styles.footerLinks}>
            <button type="button" onClick={onOpenSettings}>
              <Settings size={14} />
              Settings
            </button>
            <button type="button" onClick={onOpenHelp}>
              <CircleHelp size={14} />
              Help
            </button>
            <span className={styles.paletteHint}>
              <kbd>{modifierLabel} Shift P</kbd> commands
            </span>
          </div>
          <div className={styles.footerMeta}>
            <span>v{appInfo?.version ?? "-"}</span>
            <span>nanos world API {appInfo?.apiRevision ?? "-"}</span>
          </div>
        </footer>
      </aside>

      <main className={styles.recent}>
        <div className={styles.recentHeader}>
          <h2>Recent projects</h2>
          {recentProjects.length > 0 && (
            <div className={styles.search}>
              <TextInput
                value={filter}
                onValueChange={setFilter}
                placeholder="Filter projects"
                aria-label="Filter recent projects"
                leading={<Search size={15} />}
              />
            </div>
          )}
        </div>

        {recentProjects.length === 0 ? (
          <div className={styles.empty}>
            <Sparkles className={styles.emptyIcon} size={30} />
            <h3>No projects yet</h3>
            <p>Create your first project and the wizard will set up a ready-to-run nanos world package for you.</p>
            <Button variant="primary" icon={<Plus size={16} />} onClick={onNewProject}>
              Create a project
            </Button>
          </div>
        ) : visibleProjects.length === 0 ? (
          <div className={styles.empty}>
            <h3>Nothing matches "{filter}"</h3>
            <p>Try a different name or part of the path.</p>
          </div>
        ) : (
          <ul className={styles.list} role="list">
            {visibleProjects.map((project) => (
              <li key={project.path} className={styles.item} data-missing={!project.exists}>
                <button
                  className={styles.itemMain}
                  onClick={() => onOpenProject(project.path)}
                  title={project.exists ? `Open ${project.path}` : "This project file no longer exists"}
                >
                  <span className={styles.avatar}>{project.name.slice(0, 1).toUpperCase()}</span>
                  <span className={styles.itemText}>
                    <span className={styles.itemName}>
                      {project.name}
                      {!project.exists && <span className={styles.missing}>Missing</span>}
                    </span>
                    <span className={styles.itemPath}>
                      <bdi>{project.path}</bdi>
                    </span>
                  </span>
                </button>
                <span className={styles.itemTime}>{formatRelativeTime(project.lastOpened)}</span>
                <span className={styles.itemActions}>
                  {project.exists && (
                    <Button
                      variant="ghost"
                      size="small"
                      icon={<FolderSearch size={15} />}
                      title="Show in file manager"
                      aria-label={`Show ${project.name} in file manager`}
                      onClick={() => revealItemInDir(project.path)}
                    />
                  )}
                  <Button
                    variant="ghost"
                    size="small"
                    icon={<X size={15} />}
                    title="Remove from list (files stay untouched)"
                    aria-label={`Remove ${project.name} from recent projects`}
                    onClick={() => onRemoveRecent(project.path)}
                  />
                </span>
              </li>
            ))}
          </ul>
        )}
      </main>
    </div>
  );
}
