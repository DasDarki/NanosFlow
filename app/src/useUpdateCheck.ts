import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { useEffect } from "react";
import { useToasts } from "./components/Toasts";

export function useUpdateCheck() {
  const { notify } = useToasts();

  useEffect(() => {
    if (!import.meta.env.PROD) {
      return;
    }
    let cancelled = false;
    void check()
      .then((update) => {
        if (!update || cancelled) {
          return;
        }
        notify({
          kind: "info",
          title: `NanosFlow ${update.version} is available`,
          message: `You are on ${update.currentVersion}. The update installs and restarts NanosFlow.`,
          action: {
            label: "Install and restart",
            run: async () => {
              try {
                await update.downloadAndInstall();
                await relaunch();
              } catch (error) {
                notify({ kind: "error", title: "The update failed", message: String(error) });
              }
            },
          },
        });
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [notify]);
}
