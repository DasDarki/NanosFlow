import type { ButtonHTMLAttributes, ReactNode } from "react";
import styles from "./Button.module.css";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "small" | "medium" | "large";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  icon?: ReactNode;
  busy?: boolean;
}

export function Button({
  variant = "secondary",
  size = "medium",
  icon,
  busy = false,
  children,
  className,
  disabled,
  ...rest
}: ButtonProps) {
  const classes = [
    styles.button,
    styles[variant],
    size !== "medium" ? styles[size] : "",
    children ? "" : styles.iconOnly,
    className ?? "",
  ].join(" ");

  return (
    <button className={classes} disabled={disabled || busy} {...rest}>
      {busy ? <span className={styles.spinner} aria-hidden /> : icon}
      {children}
    </button>
  );
}
