import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { CircleAlert, CircleCheck, FolderSearch, Hammer, LoaderCircle, TriangleAlert, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { Button } from "../components/Button";
import type { BuildProblem, BuildReport, CommandError, PackageReport } from "../types";
import styles from "./BuildPanel.module.css";

export type BuildState =
  | { status: "building"; previous: BuildReport | null }
  | { status: "done"; report: BuildReport }
  | { status: "failed"; error: CommandError };

interface BuildPanelProps {
  state: BuildState;
  onClose: () => void;
  onRebuild: () => void;
  onSelectProblem: (problem: BuildProblem) => void;
}

function plural(count: number, singular: string, pluralForm = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : pluralForm}`;
}

function headline(state: BuildState): { tone: "busy" | "success" | "warning" | "error"; title: string } {
  if (state.status === "building") {
    return { tone: "busy", title: "Building Lua" };
  }
  if (state.status === "failed") {
    return { tone: "error", title: state.error.title };
  }
  const { errorCount, warningCount } = state.report;
  if (errorCount > 0) {
    return { tone: "error", title: `Build failed with ${plural(errorCount, "error")}` };
  }
  if (warningCount > 0) {
    return { tone: "warning", title: `Build finished with ${plural(warningCount, "warning")}` };
  }
  return { tone: "success", title: "Build finished" };
}

function packageSummary(report: PackageReport): string {
  if (!report.written) {
    return "Nothing was written. Fix the errors below and build again, the previous output stays untouched.";
  }
  const parts = [plural(report.fileCount, "file")];
  parts.push(report.changedFiles === 0 ? "all up to date" : `${report.changedFiles} changed`);
  if (report.removedFiles > 0) {
    parts.push(`${report.removedFiles} removed`);
  }
  return parts.join(", ");
}

function groupByGraph(problems: BuildProblem[]): [string, BuildProblem[]][] {
  const groups = new Map<string, BuildProblem[]>();
  for (const problem of problems) {
    const key = problem.graph?.location ?? "Package";
    groups.set(key, [...(groups.get(key) ?? []), problem]);
  }
  return [...groups.entries()];
}

function ProblemRow({ problem, onSelect }: { problem: BuildProblem; onSelect: (problem: BuildProblem) => void }) {
  return (
    <button
      type="button"
      className={styles.problem}
      data-severity={problem.severity}
      onClick={() => onSelect(problem)}
      disabled={!problem.graph}
      title={problem.graph ? (problem.node ? "Open the graph and show the node" : "Open the graph") : undefined}
    >
      {problem.severity === "error" ? <CircleAlert size={14} /> : <TriangleAlert size={14} />}
      <span>
        {problem.message}
        {problem.hint && <em>{problem.hint}</em>}
      </span>
    </button>
  );
}

export function BuildPanel({ state, onClose, onRebuild, onSelectProblem }: BuildPanelProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const { tone, title } = headline(state);
  const report = state.status === "done" ? state.report : state.status === "building" ? state.previous : null;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    panelRef.current?.focus();
  }, []);

  return (
    <div ref={panelRef} className={styles.panel} role="dialog" aria-label="Build result" tabIndex={-1}>
      <header className={styles.header}>
        <span className={styles.statusIcon} data-tone={tone}>
          {tone === "busy" && <LoaderCircle size={18} className={styles.spin} />}
          {tone === "success" && <CircleCheck size={18} />}
          {tone === "warning" && <TriangleAlert size={18} />}
          {tone === "error" && <CircleAlert size={18} />}
        </span>
        <div className={styles.titles}>
          <h2>{title}</h2>
          {report && <code title={report.outputDirectory}>{report.outputDirectory}</code>}
        </div>
        <Button
          variant="ghost"
          size="small"
          icon={<Hammer size={14} />}
          onClick={onRebuild}
          busy={state.status === "building"}
          title="Build again (Ctrl+B)"
        />
        <Button variant="ghost" size="small" icon={<X size={15} />} onClick={onClose} title="Close (Esc)" />
      </header>

      <div className={styles.body}>
        {state.status === "failed" && <p className={styles.failure}>{state.error.message}</p>}
        {report?.packages.map((pkg) => (
          <section key={pkg.folder} className={styles.package} data-written={pkg.written}>
            <div className={styles.packageHeader}>
              <div>
                <strong>{pkg.title}</strong>
                <span className={styles.folder}>{pkg.folder}</span>
              </div>
              {pkg.written && (
                <Button
                  variant="ghost"
                  size="small"
                  icon={<FolderSearch size={14} />}
                  onClick={() => void revealItemInDir(`${pkg.outputDirectory}/Package.toml`)}
                >
                  Show output
                </Button>
              )}
            </div>
            <p className={styles.summary}>{packageSummary(pkg)}</p>
            {groupByGraph(pkg.problems).map(([location, problems]) => (
              <div key={location} className={styles.group}>
                <h3>{location}</h3>
                {problems.map((problem, index) => (
                  <ProblemRow key={`${problem.code}-${problem.node ?? ""}-${index}`} problem={problem} onSelect={onSelectProblem} />
                ))}
              </div>
            ))}
          </section>
        ))}
        {report && report.packages.length === 0 && <p className={styles.summary}>This project has no packages yet.</p>}
      </div>
    </div>
  );
}
