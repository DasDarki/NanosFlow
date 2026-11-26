import { IssueMessage } from "../../components/Field";
import { joinPath } from "../../format";
import type { NewProjectOptions } from "../../types";
import { STEPS } from "../steps";
import styles from "../Wizard.module.css";
import type { StepProps } from "./StepProps";

const PACKAGE_TYPE_LABELS = { "game-mode": "Game Mode", script: "Script", map: "Map" } as const;

function buildTree(options: NewProjectOptions): string {
  const folder = options.folderName.trim();
  const lines = [
    `${folder}/`,
    `├── ${folder}.nfproj`,
    "├── .gitattributes",
    "├── .gitignore",
    ...(options.initializeGit ? ["├── .git/"] : []),
    "└── packages/",
    `    └── ${options.packageId}/`,
    "        ├── package.toml",
    "        └── graphs/",
  ];
  options.contexts.forEach((context, index) => {
    const isLast = index === options.contexts.length - 1;
    const branch = isLast ? "└──" : "├──";
    const indent = isLast ? "    " : "│   ";
    lines.push(`            ${branch} ${context}/`);
    lines.push(`            ${indent}└── Main/`);
    lines.push(`            ${indent}    ├── graph.toml`);
    lines.push(`            ${indent}    └── nodes/`);
  });
  return lines.join("\n");
}

export function ReviewStep({ draft, goToStep }: StepProps) {
  const { options, issues } = draft;
  const sortedIssues = [...issues].sort((left, right) => (left.severity === right.severity ? 0 : left.severity === "error" ? -1 : 1));

  const rows: { label: string; value: string; step: (typeof STEPS)[number]["id"]; mono?: boolean }[] = [
    { label: "Name", value: options.name || "-", step: "project" },
    { label: "Location", value: joinPath(options.location, options.folderName), step: "project", mono: true },
    { label: "Package", value: options.packageId || "-", step: "package", mono: true },
    { label: "Type", value: PACKAGE_TYPE_LABELS[options.packageType], step: "package" },
    { label: "Author", value: options.author || "-", step: "package" },
    { label: "Version", value: `${options.version} (compat ${options.compatibilityVersion})`, step: "package" },
    { label: "Contexts", value: options.contexts.map((context) => context[0].toUpperCase() + context.slice(1)).join(", ") || "-", step: "scripts" },
    { label: "API", value: options.apiChannel === "stable" ? "Stable" : "Latest", step: "scripts" },
    { label: "Git", value: options.initializeGit ? "New repository" : "No repository", step: "scripts" },
  ];

  return (
    <div className={styles.stepBody}>
      {sortedIssues.length > 0 && (
        <div className={styles.issueList}>
          {sortedIssues.map((issue) => {
            const step = STEPS.find((candidate) => candidate.fields.includes(issue.field));
            return (
              <div key={`${issue.field}-${issue.message}`} className={styles.issueRow}>
                <IssueMessage issue={issue} />
                {step && (
                  <button className={styles.summaryEdit} onClick={() => goToStep(step.id)}>
                    {issue.severity === "error" ? "Fix" : "Change"}
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}

      <div className={styles.reviewGrid}>
        <dl className={styles.summary}>
          {rows.map((row) => (
            <div key={row.label} className={styles.summaryRow}>
              <dt>{row.label}</dt>
              <dd className={row.mono ? "mono" : undefined}>
                {row.value}
                <button className={styles.summaryEdit} onClick={() => goToStep(row.step)} aria-label={`Edit ${row.label}`}>
                  Edit
                </button>
              </dd>
            </div>
          ))}
        </dl>
        <pre className={styles.tree} aria-label="Files that will be created">
          {buildTree(options)}
        </pre>
      </div>
    </div>
  );
}
