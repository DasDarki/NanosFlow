import { CircleAlert, CircleCheck, Info, TriangleAlert, X } from "lucide-react";
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import type { CommandError } from "../types";
import { Button } from "./Button";
import styles from "./Toasts.module.css";

export type ToastKind = "success" | "info" | "warning" | "error";

export interface ToastAction {
  label: string;
  run: () => Promise<void>;
}

interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  message?: string;
  details?: string[];
  action?: ToastAction;
}

interface ToastApi {
  notify: (toast: Omit<Toast, "id">) => void;
  reportError: (error: CommandError) => void;
}

const ToastContext = createContext<ToastApi | null>(null);
const AUTO_DISMISS_MILLISECONDS = 5000;

let nextToastId = 1;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const dismiss = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const notify = useCallback(
    (toast: Omit<Toast, "id">) => {
      const id = nextToastId++;
      setToasts((current) => [...current.slice(-3), { ...toast, id }]);
      if (!toast.action && (toast.kind === "success" || toast.kind === "info")) {
        window.setTimeout(() => dismiss(id), AUTO_DISMISS_MILLISECONDS);
      }
    },
    [dismiss],
  );

  const reportError = useCallback(
    (error: CommandError) =>
      notify({
        kind: "error",
        title: error.title,
        message: error.message,
        details: error.issues.filter((issue) => issue.severity === "error").map((issue) => issue.message),
      }),
    [notify],
  );

  const value = useMemo(() => ({ notify, reportError }), [notify, reportError]);

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className={styles.host} aria-live="polite">
        {toasts.map((toast) => (
          <ToastView key={toast.id} toast={toast} onDismiss={() => dismiss(toast.id)} />
        ))}
      </div>
    </ToastContext.Provider>
  );
}

function ToastView({ toast, onDismiss }: { toast: Toast; onDismiss: () => void }) {
  const [running, setRunning] = useState(false);
  const runAction = async (action: ToastAction) => {
    setRunning(true);
    try {
      await action.run();
    } finally {
      setRunning(false);
    }
  };
  const Icon = { success: CircleCheck, info: Info, warning: TriangleAlert, error: CircleAlert }[toast.kind];
  return (
    <div className={styles.toast} data-kind={toast.kind} role={toast.kind === "error" ? "alert" : "status"}>
      <Icon className={styles.icon} size={18} />
      <div className={styles.body}>
        <span className={styles.title}>{toast.title}</span>
        {toast.message && <span className={styles.message}>{toast.message}</span>}
        {toast.details && toast.details.length > 0 && (
          <ul className={styles.issues}>
            {toast.details.map((detail) => (
              <li key={detail}>{detail}</li>
            ))}
          </ul>
        )}
        {toast.action && (
          <div className={styles.actions}>
            <Button variant="primary" size="small" busy={running} onClick={() => toast.action && void runAction(toast.action)}>
              {toast.action.label}
            </Button>
          </div>
        )}
      </div>
      <button className={styles.close} onClick={onDismiss} aria-label="Dismiss notification">
        <X size={14} />
      </button>
    </div>
  );
}

export function useToasts(): ToastApi {
  const context = useContext(ToastContext);
  if (!context) {
    throw new Error("useToasts must be used inside a ToastProvider");
  }
  return context;
}
