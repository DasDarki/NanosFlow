import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { CommandRegistryProvider } from "./commands/registry";
import { ToastProvider } from "./components/Toasts";
import { PreferencesProvider } from "./settings/preferences";
import "./styles/global.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ToastProvider>
      <PreferencesProvider>
        <CommandRegistryProvider>
          <App />
        </CommandRegistryProvider>
      </PreferencesProvider>
    </ToastProvider>
  </StrictMode>,
);
