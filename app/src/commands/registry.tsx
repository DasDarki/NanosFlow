import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

export type CommandGroup = "Project" | "Editor" | "Graphs" | "Panels" | "Run" | "Session" | "App" | "Help";

export interface Command {
  id: string;
  title: string;
  group: CommandGroup;
  shortcut?: string;
  keywords?: string;
  detail?: string;
  run: () => void;
}

interface RegistryApi {
  register: (owner: string, commands: Command[]) => void;
  unregister: (owner: string) => void;
  commands: () => Command[];
  version: number;
}

const RegistryContext = createContext<RegistryApi | null>(null);

export function CommandRegistryProvider({ children }: { children: ReactNode }) {
  const owners = useRef(new Map<string, Command[]>());
  const [version, setVersion] = useState(0);

  const register = useCallback((owner: string, commands: Command[]) => {
    owners.current.set(owner, commands);
    setVersion((value) => value + 1);
  }, []);

  const unregister = useCallback((owner: string) => {
    if (owners.current.delete(owner)) {
      setVersion((value) => value + 1);
    }
  }, []);

  const commands = useCallback(() => [...owners.current.values()].flat(), []);

  const value = useMemo(() => ({ register, unregister, commands, version }), [register, unregister, commands, version]);
  return <RegistryContext.Provider value={value}>{children}</RegistryContext.Provider>;
}

export function useCommandRegistry(): RegistryApi {
  const registry = useContext(RegistryContext);
  if (!registry) {
    throw new Error("useCommandRegistry must be used inside a CommandRegistryProvider");
  }
  return registry;
}

export function useCommands(owner: string, commands: Command[]) {
  const { register, unregister } = useCommandRegistry();
  const latest = useRef(commands);
  latest.current = commands;
  const signature = commands.map((command) => `${command.id}|${command.title}|${command.shortcut ?? ""}|${command.detail ?? ""}`).join("\n");

  useEffect(() => {
    register(
      owner,
      latest.current.map((command) => ({ ...command, run: () => latest.current.find((current) => current.id === command.id)?.run() })),
    );
  }, [owner, signature, register]);

  useEffect(() => () => unregister(owner), [owner, unregister]);
}
