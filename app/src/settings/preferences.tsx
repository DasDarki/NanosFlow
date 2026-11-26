import { listen } from "@tauri-apps/api/event";
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { api, toCommandError } from "../api";
import { useToasts } from "../components/Toasts";
import type { Preferences } from "../types";

export const DEFAULT_PREFERENCES: Preferences = {
  interfaceScale: 100,
  buttonLabels: true,
  nodeList: "flat",
  minimap: true,
  debuggerPort: 47615,
  apiAutoUpdate: true,
};

interface PreferencesApi {
  preferences: Preferences;
  update: (change: Partial<Preferences>) => Promise<void>;
}

const PreferencesContext = createContext<PreferencesApi>({ preferences: DEFAULT_PREFERENCES, update: async () => undefined });

export function PreferencesProvider({ children }: { children: ReactNode }) {
  const { reportError } = useToasts();
  const [preferences, setPreferences] = useState<Preferences>(DEFAULT_PREFERENCES);

  useEffect(() => {
    let active = true;
    void api
      .preferences()
      .then((loaded) => active && setPreferences(loaded))
      .catch(() => undefined);
    const subscription = listen<Preferences>("preferences", ({ payload }) => setPreferences(payload));
    return () => {
      active = false;
      void subscription.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty("zoom", String(preferences.interfaceScale / 100));
    root.dataset.buttonLabels = String(preferences.buttonLabels);
  }, [preferences.interfaceScale, preferences.buttonLabels]);

  const update = useCallback(
    async (change: Partial<Preferences>) => {
      const next = { ...preferences, ...change };
      setPreferences(next);
      try {
        setPreferences(await api.updatePreferences(next));
      } catch (error) {
        reportError(toCommandError(error));
        setPreferences(await api.preferences().catch(() => preferences));
      }
    },
    [preferences, reportError],
  );

  const value = useMemo(() => ({ preferences, update }), [preferences, update]);
  return <PreferencesContext.Provider value={value}>{children}</PreferencesContext.Provider>;
}

export function usePreferences(): PreferencesApi {
  return useContext(PreferencesContext);
}
