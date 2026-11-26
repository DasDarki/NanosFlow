import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { FieldIssue, NewProjectOptions, OptionField, ProjectTemplate } from "../types";

const VALIDATION_DELAY_MILLISECONDS = 120;
const DEFAULT_MAP_ASSET = "nanos-world::BlankMap";

export interface ProjectDraft {
  options: NewProjectOptions;
  issues: FieldIssue[];
  validated: boolean;
  isFolderLinked: boolean;
  isPackageIdLinked: boolean;
  update: <K extends OptionField>(field: K, value: NewProjectOptions[K]) => void;
  setName: (name: string) => void;
  setTemplate: (template: ProjectTemplate) => void;
  relinkFolder: () => void;
  relinkPackageId: () => void;
  issueFor: (field: OptionField) => FieldIssue | undefined;
}

export function useProjectDraft(initial: NewProjectOptions): ProjectDraft {
  const [options, setOptions] = useState(initial);
  const [issues, setIssues] = useState<FieldIssue[]>([]);
  const [validated, setValidated] = useState(false);
  const [isFolderLinked, setFolderLinked] = useState(true);
  const [isPackageIdLinked, setPackageIdLinked] = useState(true);
  const validationRound = useRef(0);

  useEffect(() => {
    const round = ++validationRound.current;
    const timer = window.setTimeout(async () => {
      const result = await api.validateNewProject(options);
      if (round === validationRound.current) {
        setIssues(result);
        setValidated(true);
      }
    }, VALIDATION_DELAY_MILLISECONDS);
    return () => window.clearTimeout(timer);
  }, [options]);

  const update = useCallback(<K extends OptionField>(field: K, value: NewProjectOptions[K]) => {
    setValidated(false);
    setOptions((current) => ({ ...current, [field]: value }));
    if (field === "folderName") {
      setFolderLinked(false);
    }
    if (field === "packageId") {
      setPackageIdLinked(false);
    }
  }, []);

  const setName = useCallback(
    async (name: string) => {
      setValidated(false);
      setOptions((current) => ({ ...current, name }));
      const slug = await api.slugify(name);
      setOptions((current) => {
        if (current.name !== name) {
          return current;
        }
        return {
          ...current,
          folderName: isFolderLinked ? slug : current.folderName,
          packageId: isPackageIdLinked ? slug : current.packageId,
        };
      });
    },
    [isFolderLinked, isPackageIdLinked],
  );

  const relink = useCallback(async (field: "folderName" | "packageId") => {
    const slug = await api.slugify(options.name);
    setOptions((current) => ({ ...current, [field]: slug }));
    if (field === "folderName") {
      setFolderLinked(true);
    } else {
      setPackageIdLinked(true);
    }
  }, [options.name]);

  const setTemplate = useCallback((template: ProjectTemplate) => {
    setValidated(false);
    setOptions((current) => {
      if (current.template === template) {
        return current;
      }
      const packageType = template === "game-mode" ? "game-mode" : template === "map" ? "map" : "script";
      return {
        ...current,
        template,
        packageType,
        contexts: template === "blank" ? ["server"] : ["server", "client"],
        mapAsset: template === "map" ? (current.mapAsset ?? DEFAULT_MAP_ASSET) : current.mapAsset,
      };
    });
  }, []);

  const issueFor = useCallback(
    (field: OptionField) =>
      issues.find((issue) => issue.field === field && issue.severity === "error") ??
      issues.find((issue) => issue.field === field),
    [issues],
  );

  return useMemo(
    () => ({
      options,
      issues,
      validated,
      isFolderLinked,
      isPackageIdLinked,
      update,
      setName,
      setTemplate,
      relinkFolder: () => relink("folderName"),
      relinkPackageId: () => relink("packageId"),
      issueFor,
    }),
    [options, issues, validated, isFolderLinked, isPackageIdLinked, update, setName, setTemplate, relink, issueFor],
  );
}
