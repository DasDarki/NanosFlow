import { GitBranch, Layers, Monitor, Server } from "lucide-react";
import { ChoiceCard, ChoiceGrid, Segmented, ToggleRow } from "../../components/Choice";
import { IssueMessage } from "../../components/Field";
import type { ApiChannel, ScriptContext } from "../../types";
import styles from "../Wizard.module.css";
import type { StepProps } from "./StepProps";

const CONTEXTS: { id: ScriptContext; title: string; description: string; icon: typeof Server; tint: string }[] = [
  {
    id: "server",
    title: "Server",
    description: "Authoritative game logic: spawning, damage, rules and saving data.",
    icon: Server,
    tint: "var(--context-server)",
  },
  {
    id: "client",
    title: "Client",
    description: "Runs on every player: input, HUD, UI, sounds and camera.",
    icon: Monitor,
    tint: "var(--context-client)",
  },
  {
    id: "shared",
    title: "Shared",
    description: "Loaded on both sides. Ideal for config, constants and helpers.",
    icon: Layers,
    tint: "var(--context-shared)",
  },
];

const CHANNELS: { value: ApiChannel; label: string }[] = [
  { value: "stable", label: "Stable" },
  { value: "latest", label: "Latest" },
];

export function ScriptsStep({ draft, visibleIssue }: StepProps) {
  const { options } = draft;
  const contextsIssue = visibleIssue("contexts");

  function toggleContext(context: ScriptContext) {
    const contexts = options.contexts.includes(context)
      ? options.contexts.filter((existing) => existing !== context)
      : CONTEXTS.map((entry) => entry.id).filter((id) => id === context || options.contexts.includes(id));
    draft.update("contexts", contexts);
  }

  return (
    <div className={styles.stepBody}>
      <span className={styles.sectionLabel}>Script contexts</span>
      <ChoiceGrid columns={3}>
        {CONTEXTS.map((context) => (
          <ChoiceCard
            key={context.id}
            role="checkbox"
            title={context.title}
            description={context.description}
            icon={<context.icon size={18} />}
            tint={context.tint}
            selected={options.contexts.includes(context.id)}
            onSelect={() => toggleContext(context.id)}
          />
        ))}
      </ChoiceGrid>
      {contextsIssue && <IssueMessage issue={contextsIssue} />}

      <span className={styles.sectionLabel}>nanos world API</span>
      <div>
        <Segmented
          label="API channel"
          value={options.apiChannel}
          options={CHANNELS}
          onChange={(value) => draft.update("apiChannel", value)}
        />
      </div>

      <span className={styles.sectionLabel}>Version control</span>
      <ToggleRow
        title="Create a Git repository"
        description="Runs git init in the project folder, with a ready .gitignore and .gitattributes."
        checked={options.initializeGit}
        onChange={(checked) => draft.update("initializeGit", checked)}
      />
      <div className={styles.callout}>
        <GitBranch size={18} />
        <div>
          <strong>Built for teams.</strong> NanosFlow stores projects so that working in parallel does not cause merge
          conflicts:
          <ul>
            <li>Every node is its own small file with a random id, so two people editing different nodes never touch the same file.</li>
            <li>Packages and graphs are found from folders. There is no central list everyone has to edit.</li>
            <li>Line endings are always LF, so Windows and Linux machines produce identical files.</li>
            <li>Editor state and generated Lua stay local and are ignored by Git.</li>
          </ul>
        </div>
      </div>
    </div>
  );
}
