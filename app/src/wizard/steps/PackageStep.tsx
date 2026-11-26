import { Link2, Link2Off } from "lucide-react";
import { Segmented } from "../../components/Choice";
import { Field, TextInput } from "../../components/Field";
import type { PackageType } from "../../types";
import styles from "../Wizard.module.css";
import type { StepProps } from "./StepProps";

const PACKAGE_TYPES: { value: PackageType; label: string }[] = [
  { value: "game-mode", label: "Game Mode" },
  { value: "script", label: "Script" },
  { value: "map", label: "Map" },
];

const PACKAGE_TYPE_HINTS: Record<PackageType, string> = {
  "game-mode": "Only one game mode can run at a time. Set it as game_mode in the server Config.toml.",
  script: "Any number of scripts can run together. Add it to packages in the server Config.toml.",
  map: "Set it as map in the server Config.toml. Map packages can run scripts too.",
};

export function PackageStep({ draft, visibleIssue, markTouched }: StepProps) {
  const { options } = draft;

  return (
    <div className={styles.stepBody}>
      <Field
        label="Package id"
        htmlFor="package-id"
        issue={visibleIssue("packageId")}
        hint="Unique id and folder name of the package on the server and in the Vault. Lowercase letters, digits and '-'."
      >
        <TextInput
          id="package-id"
          mono
          value={options.packageId}
          placeholder="my-deathmatch"
          issue={visibleIssue("packageId")}
          onValueChange={(value) => draft.update("packageId", value)}
          onBlur={() => markTouched("packageId")}
          trailing={
            <button
              type="button"
              className={styles.linkButton}
              data-linked={draft.isPackageIdLinked}
              title={draft.isPackageIdLinked ? "Linked to the project name" : "Link to the project name again"}
              aria-label="Link package id to project name"
              onClick={draft.relinkPackageId}
            >
              {draft.isPackageIdLinked ? <Link2 size={15} /> : <Link2Off size={15} />}
            </button>
          }
        />
      </Field>

      <Field label="Package type" hint={PACKAGE_TYPE_HINTS[options.packageType]}>
        <div>
          <Segmented
            label="Package type"
            value={options.packageType}
            options={PACKAGE_TYPES}
            onChange={(value) => draft.update("packageType", value)}
          />
        </div>
      </Field>

      {options.packageType === "map" && (
        <Field label="Map asset" htmlFor="map-asset" issue={visibleIssue("mapAsset")} hint="Asset path in the format ASSET_PACK::ASSET_KEY.">
          <TextInput
            id="map-asset"
            mono
            value={options.mapAsset ?? ""}
            placeholder="nanos-world::BlankMap"
            issue={visibleIssue("mapAsset")}
            onValueChange={(value) => draft.update("mapAsset", value)}
            onBlur={() => markTouched("mapAsset")}
          />
        </Field>
      )}

      <div className={styles.row}>
        <Field label="Author" htmlFor="package-author" issue={visibleIssue("author")}>
          <TextInput
            id="package-author"
            value={options.author}
            issue={visibleIssue("author")}
            onValueChange={(value) => draft.update("author", value)}
            onBlur={() => markTouched("author")}
          />
        </Field>
        <Field label="Version" htmlFor="package-version" issue={visibleIssue("version")} hint="MAJOR.MINOR.PATCH">
          <TextInput
            id="package-version"
            mono
            value={options.version}
            issue={visibleIssue("version")}
            onValueChange={(value) => draft.update("version", value)}
            onBlur={() => markTouched("version")}
          />
        </Field>
      </div>

      <Field
        label="Compatibility version"
        htmlFor="compatibility-version"
        issue={visibleIssue("compatibilityVersion")}
        hint="The nanos world version your scripts target. Newer servers keep old behavior for packages with an older value."
      >
        <TextInput
          id="compatibility-version"
          mono
          value={options.compatibilityVersion}
          issue={visibleIssue("compatibilityVersion")}
          onValueChange={(value) => draft.update("compatibilityVersion", value)}
          onBlur={() => markTouched("compatibilityVersion")}
        />
      </Field>
    </div>
  );
}
