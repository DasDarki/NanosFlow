import { AlertTriangle, CircleAlert } from "lucide-react";
import type { InputHTMLAttributes, ReactNode } from "react";
import type { FieldIssue } from "../types";
import styles from "./Field.module.css";

interface FieldProps {
  label: string;
  htmlFor?: string;
  optional?: boolean;
  hint?: ReactNode;
  issue?: FieldIssue;
  children: ReactNode;
}

export function Field({ label, htmlFor, optional, hint, issue, children }: FieldProps) {
  return (
    <div className={styles.field}>
      <div className={styles.labelRow}>
        <label className={styles.label} htmlFor={htmlFor}>
          {label}
        </label>
        {optional && <span className={styles.optional}>Optional</span>}
      </div>
      {children}
      {issue ? <IssueMessage issue={issue} /> : hint && <p className={styles.hint}>{hint}</p>}
    </div>
  );
}

export function IssueMessage({ issue }: { issue: FieldIssue }) {
  const Icon = issue.severity === "error" ? CircleAlert : AlertTriangle;
  return (
    <p className={styles.message} data-severity={issue.severity} role={issue.severity === "error" ? "alert" : undefined}>
      <Icon size={14} />
      <span>{issue.message}</span>
    </p>
  );
}

interface TextInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "onChange"> {
  issue?: FieldIssue;
  mono?: boolean;
  leading?: ReactNode;
  trailing?: ReactNode;
  onValueChange: (value: string) => void;
}

export function TextInput({ issue, mono, leading, trailing, onValueChange, className, ...rest }: TextInputProps) {
  return (
    <div className={styles.control} data-state={issue?.severity}>
      {leading && <span className={styles.adornment}>{leading}</span>}
      <input
        className={`${styles.input} ${mono ? styles.mono : ""} ${className ?? ""}`}
        spellCheck={false}
        autoComplete="off"
        aria-invalid={issue?.severity === "error"}
        onChange={(event) => onValueChange(event.target.value)}
        {...rest}
      />
      {trailing && <span className={styles.trailing}>{trailing}</span>}
    </div>
  );
}
