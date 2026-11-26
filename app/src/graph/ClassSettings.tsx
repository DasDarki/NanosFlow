import { Plug, Plus, X } from "lucide-react";
import { IssueMessage } from "../components/Field";
import type { FieldIssue, ScriptContext } from "../types";
import styles from "./ClassSettings.module.css";

interface ClassSettingsProps {
  name: string;
  context: ScriptContext;
  parent: string;
  description: string;
  apiParents: string[];
  projectParents: string[];
  issue: FieldIssue | null;
  interfaces: string[];
  availableInterfaces: string[];
  onInterfacesChange: (interfaces: string[]) => void;
  exported: boolean;
  onExportedChange: (exported: boolean) => void;
  onParentChange: (parent: string) => void;
  onDescriptionChange: (description: string) => void;
}

export function ClassSettings({
  name,
  context,
  parent,
  description,
  apiParents,
  projectParents,
  issue,
  interfaces,
  availableInterfaces,
  onInterfacesChange,
  exported,
  onExportedChange,
  onParentChange,
  onDescriptionChange,
}: ClassSettingsProps) {
  return (
    <div className={styles.settings}>
      <div className={styles.kind}>
        Class <span data-context={context}>{context}</span>
      </div>
      <h3 className={styles.title}>{name}</h3>
      <textarea
        className={styles.description}
        value={description}
        placeholder="What is this class for? Shown on its Spawn node and in tooltips."
        rows={2}
        onChange={(event) => onDescriptionChange(event.target.value)}
      />
      <label className={styles.parentRow}>
        <span>Inherits from</span>
        <select className={styles.select} value={parent} onChange={(event) => onParentChange(event.target.value)}>
          {projectParents.length > 0 && (
            <optgroup label="Your classes">
              {projectParents.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </optgroup>
          )}
          <optgroup label="nanos world">
            {apiParents.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </optgroup>
        </select>
      </label>
      {issue && <IssueMessage issue={issue} />}
      <label className={styles.export}>
        <input type="checkbox" checked={exported} onChange={(event) => onExportedChange(event.target.checked)} />
        <span>
          <strong>Export to other packages</strong>
          <span>Packages that require this package can spawn {name}, cast to it, call its methods and inherit from it.</span>
        </span>
      </label>
      <div className={styles.interfaces}>
        <span className={styles.interfacesTitle}>Implements</span>
        <div className={styles.chips}>
          {interfaces.map((interfaceName) => (
            <span key={interfaceName} className={styles.chip}>
              <Plug size={12} />
              {interfaceName}
              <button
                type="button"
                aria-label={`Stop implementing ${interfaceName}`}
                onClick={() => onInterfacesChange(interfaces.filter((other) => other !== interfaceName))}
              >
                <X size={12} />
              </button>
            </span>
          ))}
          {availableInterfaces.some((candidate) => !interfaces.includes(candidate)) && (
            <label className={styles.addInterface}>
              <Plus size={12} />
              <select
                value=""
                aria-label="Implement an interface"
                onChange={(event) => event.target.value && onInterfacesChange([...interfaces, event.target.value])}
              >
                <option value="">Add interface</option>
                {availableInterfaces
                  .filter((candidate) => !interfaces.includes(candidate))
                  .map((candidate) => (
                    <option key={candidate} value={candidate}>
                      {candidate}
                    </option>
                  ))}
              </select>
            </label>
          )}
          {interfaces.length === 0 && availableInterfaces.length === 0 && (
            <span className={styles.none}>No interfaces in this package yet.</span>
          )}
        </div>
      </div>
      <p className={styles.note}>
        The events of {parent} are available in this graph, with Self typed as {name}. Spawn it anywhere in the package with{" "}
        <em>Spawn {name}</em>, and add methods with a right-click on the class in the sidebar. Changing the parent updates
        every event, spawn and cast node right away; nodes that no longer fit are flagged in Problems.
      </p>
    </div>
  );
}
