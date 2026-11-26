import { ArrowLeft, ArrowRight, Check, CircleAlert, FolderPlus } from "lucide-react";
import { useCallback, useEffect, useMemo, useState, type ComponentType } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { useToasts } from "../components/Toasts";
import { joinPath, modifierLabel } from "../format";
import type { NewProjectOptions, OptionField, ProjectView } from "../types";
import { STEPS, type StepId } from "./steps";
import { PackageStep } from "./steps/PackageStep";
import { ProjectStep } from "./steps/ProjectStep";
import { ReviewStep } from "./steps/ReviewStep";
import { ScriptsStep } from "./steps/ScriptsStep";
import type { StepProps } from "./steps/StepProps";
import { TemplateStep } from "./steps/TemplateStep";
import { useProjectDraft } from "./useProjectDraft";
import styles from "./Wizard.module.css";

const STEP_COMPONENTS: Record<StepId, ComponentType<StepProps>> = {
  template: TemplateStep,
  project: ProjectStep,
  package: PackageStep,
  scripts: ScriptsStep,
  review: ReviewStep,
};

interface NewProjectWizardProps {
  initialOptions: NewProjectOptions;
  onCancel: () => void;
  onCreated: (project: ProjectView) => void;
}

export function NewProjectWizard({ initialOptions, onCancel, onCreated }: NewProjectWizardProps) {
  const draft = useProjectDraft(initialOptions);
  const { notify, reportError } = useToasts();
  const [stepIndex, setStepIndex] = useState(0);
  const [furthestStep, setFurthestStep] = useState(0);
  const [touched, setTouched] = useState<Set<OptionField>>(new Set());
  const [attemptedSteps, setAttemptedSteps] = useState<Set<StepId>>(new Set());
  const [creating, setCreating] = useState(false);

  const step = STEPS[stepIndex];
  const isLastStep = stepIndex === STEPS.length - 1;
  const errorFields = useMemo(
    () => new Set(draft.issues.filter((issue) => issue.severity === "error").map((issue) => issue.field)),
    [draft.issues],
  );
  const stepHasErrors = useCallback(
    (index: number) => STEPS[index].fields.some((field) => errorFields.has(field)),
    [errorFields],
  );
  const hasAnyError = errorFields.size > 0;

  const markTouched = useCallback((field: OptionField) => {
    setTouched((current) => (current.has(field) ? current : new Set(current).add(field)));
  }, []);

  const visibleIssue = useCallback(
    (field: OptionField) => {
      const issue = draft.issueFor(field);
      if (!issue) {
        return undefined;
      }
      const owningStep = STEPS.find((candidate) => candidate.fields.includes(field));
      const revealed = touched.has(field) || (owningStep && attemptedSteps.has(owningStep.id));
      return revealed || issue.severity === "warning" ? issue : undefined;
    },
    [draft, touched, attemptedSteps],
  );

  const goToIndex = useCallback((index: number) => {
    setStepIndex(index);
    setFurthestStep((current) => Math.max(current, index));
  }, []);

  const goToStep = useCallback(
    (id: StepId) => goToIndex(STEPS.findIndex((candidate) => candidate.id === id)),
    [goToIndex],
  );

  const create = useCallback(async () => {
    if (hasAnyError || !draft.validated) {
      setAttemptedSteps(new Set(STEPS.map((candidate) => candidate.id)));
      const firstBroken = STEPS.findIndex((_, index) => stepHasErrors(index));
      if (firstBroken >= 0) {
        goToIndex(firstBroken);
      }
      return;
    }
    setCreating(true);
    try {
      const result = await api.createProject(draft.options);
      result.warnings.forEach((warning) => notify({ kind: "warning", title: "Project created with a warning", message: warning }));
      notify({ kind: "success", title: `${result.project.name} is ready`, message: "Your project was created." });
      onCreated(result.project);
    } catch (error) {
      reportError(toCommandError(error));
    } finally {
      setCreating(false);
    }
  }, [hasAnyError, draft.validated, draft.options, stepHasErrors, goToIndex, notify, onCreated, reportError]);

  const next = useCallback(() => {
    if (isLastStep) {
      void create();
      return;
    }
    if (stepHasErrors(stepIndex)) {
      setAttemptedSteps((current) => new Set(current).add(step.id));
      const firstField = step.fields.find((field) => errorFields.has(field));
      const input = document.querySelector<HTMLInputElement>("[aria-invalid='true']");
      input?.focus();
      if (!input && firstField) {
        markTouched(firstField);
      }
      return;
    }
    goToIndex(stepIndex + 1);
  }, [isLastStep, create, stepHasErrors, stepIndex, step, errorFields, markTouched, goToIndex]);

  const back = useCallback(() => setStepIndex((current) => Math.max(0, current - 1)), []);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape" && !creating) {
        event.preventDefault();
        onCancel();
      } else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        void create();
      } else if (event.key === "Enter" && event.target instanceof HTMLInputElement) {
        event.preventDefault();
        next();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [creating, onCancel, create, next]);

  const StepComponent = STEP_COMPONENTS[step.id];
  const projectFolder = joinPath(draft.options.location, draft.options.folderName.trim());

  return (
    <div className={styles.backdrop}>
      <div className={styles.dialog} role="dialog" aria-modal="true" aria-labelledby="wizard-heading">
        <nav className={styles.rail} aria-label="Wizard steps">
          <div className={styles.railTitle}>
            New project
            <span>
              {stepIndex + 1} of {STEPS.length}
            </span>
          </div>
          <ol className={styles.steps}>
            {STEPS.map((candidate, index) => {
              const showError = stepHasErrors(index) && (attemptedSteps.has(candidate.id) || candidate.fields.some((field) => touched.has(field)));
              const status = showError ? "error" : index < stepIndex || (index <= furthestStep && index !== stepIndex) ? "done" : "pending";
              return (
                <li key={candidate.id}>
                  <button
                    className={styles.step}
                    aria-current={index === stepIndex ? "step" : undefined}
                    data-status={status}
                    disabled={index > furthestStep}
                    onClick={() => goToIndex(index)}
                  >
                    <span className={styles.stepMarker}>
                      {status === "error" ? <CircleAlert size={14} /> : status === "done" ? <Check size={14} strokeWidth={3} /> : index + 1}
                    </span>
                    <span className={styles.stepText}>
                      <span className={styles.stepTitle}>{candidate.title}</span>
                      <span className={styles.stepSubtitle}>{candidate.subtitle}</span>
                    </span>
                  </button>
                </li>
              );
            })}
          </ol>
          <p className={styles.railFooter}>
            Enter: next step
            <br />
            {modifierLabel}+Enter: create
            <br />
            Esc: cancel
          </p>
        </nav>

        <section className={styles.content}>
          <header className={styles.header}>
            <span className={styles.eyebrow}>{step.title}</span>
            <h2 id="wizard-heading" className={styles.heading}>
              {step.heading}
            </h2>
            <p className={styles.description}>{step.description}</p>
          </header>
          <div className={styles.body} key={step.id}>
            <StepComponent draft={draft} visibleIssue={visibleIssue} markTouched={markTouched} goToStep={goToStep} />
          </div>
          <footer className={styles.footer}>
            <span className={styles.footerPath} title={projectFolder}>
              {draft.options.folderName.trim() && (
                <>
                  <FolderPlus size={14} />
                  <code>
                    <bdi>{projectFolder}</bdi>
                  </code>
                </>
              )}
            </span>
            <Button variant="ghost" onClick={onCancel} disabled={creating}>
              Cancel
            </Button>
            {stepIndex > 0 && (
              <Button icon={<ArrowLeft size={16} />} onClick={back} disabled={creating}>
                Back
              </Button>
            )}
            {isLastStep ? (
              <Button variant="primary" icon={<Check size={16} />} busy={creating} disabled={hasAnyError} onClick={create}>
                Create project
              </Button>
            ) : (
              <Button variant="primary" onClick={next}>
                Continue
                <ArrowRight size={16} />
              </Button>
            )}
          </footer>
        </section>
      </div>
    </div>
  );
}
