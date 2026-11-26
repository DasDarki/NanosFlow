import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Link2, Link2Off } from "lucide-react";
import { Button } from "../../components/Button";
import { Field, TextInput } from "../../components/Field";
import { joinPath } from "../../format";
import styles from "../Wizard.module.css";
import type { StepProps } from "./StepProps";

export function ProjectStep({ draft, visibleIssue, markTouched }: StepProps) {
  const { options } = draft;

  async function browseLocation() {
    const selected = await open({ directory: true, defaultPath: options.location, title: "Choose where to create the project" });
    if (typeof selected === "string") {
      draft.update("location", selected);
      markTouched("location");
    }
  }

  const folder = options.folderName.trim() || "…";
  return (
    <div className={styles.stepBody}>
      <Field label="Project name" htmlFor="project-name" issue={visibleIssue("name")} hint="Shown in NanosFlow and used as the package title.">
        <TextInput
          id="project-name"
          autoFocus
          value={options.name}
          placeholder="My Deathmatch"
          issue={visibleIssue("name")}
          onValueChange={draft.setName}
          onBlur={() => markTouched("name")}
        />
      </Field>

      <Field label="Location" htmlFor="project-location" issue={visibleIssue("location")}>
        <TextInput
          id="project-location"
          mono
          value={options.location}
          issue={visibleIssue("location")}
          onValueChange={(value) => draft.update("location", value)}
          onBlur={() => markTouched("location")}
          trailing={
            <Button variant="ghost" size="small" icon={<FolderOpen size={15} />} onClick={browseLocation}>
              Browse
            </Button>
          }
        />
      </Field>

      <Field
        label="Folder name"
        htmlFor="project-folder"
        issue={visibleIssue("folderName")}
        hint={draft.isFolderLinked ? "Follows the project name. Edit it to set your own." : "Set manually. Click the link icon to follow the project name again."}
      >
        <TextInput
          id="project-folder"
          mono
          value={options.folderName}
          issue={visibleIssue("folderName")}
          onValueChange={(value) => draft.update("folderName", value)}
          onBlur={() => markTouched("folderName")}
          trailing={
            <button
              type="button"
              className={styles.linkButton}
              data-linked={draft.isFolderLinked}
              title={draft.isFolderLinked ? "Linked to the project name" : "Link to the project name again"}
              aria-label="Link folder name to project name"
              onClick={draft.relinkFolder}
            >
              {draft.isFolderLinked ? <Link2 size={15} /> : <Link2Off size={15} />}
            </button>
          }
        />
      </Field>

      <div className={styles.pathPreview}>
        <span>Project file</span>
        <code>
          {joinPath(options.location, folder)}
          {options.location.includes("\\") ? "\\" : "/"}
          <mark>{folder}.nfproj</mark>
        </code>
      </div>
    </div>
  );
}
