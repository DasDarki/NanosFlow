import { open } from "@tauri-apps/plugin-dialog";
import { Check, FolderOpen, HardDrive, Library, Package, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { ChoiceCard, ChoiceGrid } from "../components/Choice";
import type { CommandError, PackageSummary, ProjectView } from "../types";
import dialogStyles from "./EntryDialog.module.css";
import styles from "./ProjectSettings.module.css";

interface ProjectSettingsProps {
  project: ProjectView;
  onClose: () => void;
  onProjectChange: (project: ProjectView) => void;
}

type OutputMode = "project" | "custom";

interface RequirementsProps {
  project: ProjectView;
  pkg: PackageSummary;
  onProjectChange: (project: ProjectView) => void;
}

function PackageRequirements({ project, pkg, onProjectChange }: RequirementsProps) {
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const others = project.packages.filter((other) => other.path !== pkg.path);
  const external = pkg.requirements.filter((requirement) => !others.some((other) => other.folder === requirement));

  const save = async (requirements: string[]) => {
    setError(null);
    try {
      onProjectChange(await api.updatePackageRequirements(project.projectFile, pkg.path, requirements));
      return true;
    } catch (reason) {
      setError(toCommandError(reason).message);
      return false;
    }
  };

  const toggle = (folder: string) =>
    void save(pkg.requirements.includes(folder) ? pkg.requirements.filter((requirement) => requirement !== folder) : [...pkg.requirements, folder]);

  const addExternal = async () => {
    const name = draft.trim();
    if (name !== "" && (await save([...pkg.requirements, name]))) {
      setDraft("");
    }
  };

  return (
    <div className={styles.requirements}>
      <div className={styles.packageName}>
        <Package size={14} />
        <span>{pkg.title}</span>
        <code>{pkg.folder}</code>
      </div>
      <div className={styles.chips}>
        {others.map((other) => {
          const required = pkg.requirements.includes(other.folder);
          const exported = other.graphs.filter((graph) => graph.exported).length;
          return (
            <button
              key={other.path}
              type="button"
              className={styles.chip}
              aria-pressed={required}
              title={exported > 0 ? `Exports ${exported} ${exported === 1 ? "function or class" : "functions and classes"}` : "Exports nothing yet"}
              onClick={() => toggle(other.folder)}
            >
              {required ? <Check size={12} /> : <Plus size={12} />}
              {other.folder}
              {exported > 0 && (
                <span className={styles.exportCount}>
                  <Library size={11} />
                  {exported}
                </span>
              )}
            </button>
          );
        })}
        {external.map((requirement) => (
          <span key={requirement} className={styles.chip} data-external>
            {requirement}
            <button
              type="button"
              className={styles.remove}
              aria-label={`Remove ${requirement}`}
              onClick={() => void save(pkg.requirements.filter((other) => other !== requirement))}
            >
              <X size={12} />
            </button>
          </span>
        ))}
        <input
          className={styles.externalInput}
          value={draft}
          placeholder="Server package, e.g. default-vehicles"
          aria-label={`Add a server package that ${pkg.folder} requires`}
          spellCheck={false}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void addExternal();
            }
          }}
        />
      </div>
      {error && <p className={styles.error}>{error}</p>}
    </div>
  );
}

export function ProjectSettings({ project, onClose, onProjectChange }: ProjectSettingsProps) {
  const [mode, setMode] = useState<OutputMode>(project.customOutputDirectory ? "custom" : "project");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CommandError | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const apply = async (directory: string | null) => {
    setBusy(true);
    setError(null);
    try {
      onProjectChange(await api.updateOutputDirectory(project.projectFile, directory));
    } catch (reason) {
      setError(toCommandError(reason));
    } finally {
      setBusy(false);
    }
  };

  const choose = async () => {
    const selected = await open({
      directory: true,
      defaultPath: project.customOutputDirectory ?? project.root,
      title: "Choose the nanos world server or its Packages folder",
    });
    if (typeof selected === "string") {
      setMode("custom");
      await apply(selected);
    }
  };

  return (
    <div className={dialogStyles.backdrop} onMouseDown={onClose}>
      <div
        className={`${dialogStyles.dialog} ${styles.dialog}`}
        role="dialog"
        aria-label="Project settings"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div>
          <h2 className={dialogStyles.title}>Project settings</h2>
          <p className={dialogStyles.location}>
            <code>{project.projectFile}</code>
          </p>
        </div>

        <section className={styles.section}>
          <h3>Build output</h3>
          <ChoiceGrid columns={2}>
            <ChoiceCard
              title="Inside the project"
              description="Writes every package to the build folder of the project. Copy or link it to your server yourself."
              icon={<Package size={17} />}
              selected={mode === "project"}
              onSelect={() => {
                setMode("project");
                if (project.customOutputDirectory) {
                  void apply(null);
                }
              }}
            />
            <ChoiceCard
              title="Server packages folder"
              description="Writes straight into your nanos world server, so a build is ready to test right away."
              icon={<HardDrive size={17} />}
              tint="#34d399"
              selected={mode === "custom"}
              onSelect={() => {
                if (project.customOutputDirectory) {
                  setMode("custom");
                } else {
                  void choose();
                }
              }}
            />
          </ChoiceGrid>

          <div className={styles.target}>
            <span className={styles.label}>Packages are written to</span>
            <code title={project.outputDirectory}>{project.outputDirectory}</code>
            {mode === "custom" && (
              <Button variant="secondary" size="small" icon={<FolderOpen size={14} />} onClick={() => void choose()} busy={busy}>
                Choose folder
              </Button>
            )}
          </div>
          {error && <p className={styles.error}>{error.message}</p>}
          <p className={styles.note}>
            Each package goes into its own folder named like the package. Picking the server folder itself also works,
            NanosFlow then uses its Packages folder. Existing folders that NanosFlow did not create are never overwritten.
            This setting is stored only on this computer and stays out of version control.
          </p>
        </section>

        {project.packages.length > 0 && (
          <section className={styles.section}>
            <h3>Package requirements</h3>
            {project.packages.map((pkg) => (
              <PackageRequirements key={pkg.path} project={project} pkg={pkg} onProjectChange={onProjectChange} />
            ))}
            <p className={styles.note}>
              nanos world loads required packages first. Exported functions of a required package appear in the node menu
              under Libraries, exported classes like your own classes. Packages from your server that are not part of this project can be added by their folder name.
            </p>
          </section>
        )}

        <div className={dialogStyles.actions}>
          <Button type="button" variant="primary" onClick={onClose}>
            Done
          </Button>
        </div>
      </div>
    </div>
  );
}
