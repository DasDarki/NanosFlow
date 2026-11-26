import { Check } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import styles from "./Choice.module.css";

interface ChoiceCardProps {
  title: string;
  description: ReactNode;
  icon: ReactNode;
  tint?: string;
  badge?: string;
  selected: boolean;
  role?: "radio" | "checkbox";
  onSelect: () => void;
}

export function ChoiceCard({ title, description, icon, tint, badge, selected, role = "radio", onSelect }: ChoiceCardProps) {
  return (
    <button
      type="button"
      role={role}
      aria-checked={selected}
      className={styles.card}
      style={{ "--tint": tint } as CSSProperties}
      onClick={onSelect}
    >
      <div className={styles.cardHeader}>
        <span className={styles.icon}>{icon}</span>
        <span className={styles.title}>{title}</span>
        {badge && !selected && <span className={styles.badge}>{badge}</span>}
      </div>
      <p className={styles.description}>{description}</p>
      {selected && (
        <span className={styles.check}>
          <Check size={12} strokeWidth={3} />
        </span>
      )}
    </button>
  );
}

export function ChoiceGrid({ columns, children }: { columns: number; children: ReactNode }) {
  return (
    <div className={styles.grid} style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}>
      {children}
    </div>
  );
}

interface SegmentedProps<T extends string> {
  value: T;
  options: { value: T; label: string; icon?: ReactNode }[];
  onChange: (value: T) => void;
  label: string;
}

export function Segmented<T extends string>({ value, options, onChange, label }: SegmentedProps<T>) {
  return (
    <div className={styles.segmented} role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          className={styles.segment}
          onClick={() => onChange(option.value)}
        >
          {option.icon}
          {option.label}
        </button>
      ))}
    </div>
  );
}

interface ToggleRowProps {
  title: string;
  description: ReactNode;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

export function ToggleRow({ title, description, checked, onChange }: ToggleRowProps) {
  return (
    <button type="button" role="switch" aria-checked={checked} className={styles.toggleRow} onClick={() => onChange(!checked)}>
      <span className={styles.toggleText}>
        <span className={styles.title}>{title}</span>
        <span className={styles.description}>{description}</span>
      </span>
      <span className={styles.switch} aria-hidden />
    </button>
  );
}
