import type { FieldIssue, OptionField } from "../../types";
import type { ProjectDraft } from "../useProjectDraft";
import type { StepId } from "../steps";

export interface StepProps {
  draft: ProjectDraft;
  visibleIssue: (field: OptionField) => FieldIssue | undefined;
  markTouched: (field: OptionField) => void;
  goToStep: (step: StepId) => void;
}
