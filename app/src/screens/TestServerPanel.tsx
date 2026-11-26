import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Copy, Download, FolderOpen, FolderSearch, HardDrive, Play, RotateCw, Server, Square, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { useFloating } from "../components/useFloating";
import { ChoiceCard, ChoiceGrid } from "../components/Choice";
import type { InstallProgress, ProjectView, ServerBranch, SteamApp, TestServerSettings, TestServerStatus } from "../types";
import styles from "./TestServerPanel.module.css";

export interface ConsoleLine {
  id: number;
  text: string;
  kind: "output" | "command" | "install" | "notice";
}

export interface InstallState {
  progress: InstallProgress | null;
  line: string | null;
  result: { success: boolean; message: string } | null;
}

export type ServerTab = "console" | "setup";

interface TestServerPanelProps {
  project: ProjectView;
  status: TestServerStatus | null;
  lines: ConsoleLine[];
  install: InstallState;
  tab: ServerTab;
  playing: boolean;
  onTabChange: (tab: ServerTab) => void;
  onStatusChange: (status: TestServerStatus) => void;
  onInstall: () => void;
  onPlay: (restart: boolean) => void;
  onStop: () => void;
  onClearConsole: () => void;
  onClose: () => void;
}

function lineTone(line: ConsoleLine): string {
  if (line.kind === "command" || line.kind === "notice") {
    return line.kind;
  }
  if (/\bERROR\b|Lua Error|attempt to |stack traceback/.test(line.text)) {
    return "error";
  }
  if (/\bWARNING\b/.test(line.text)) {
    return "warning";
  }
  if (/\bSUCCESS\b|Success!/.test(line.text)) {
    return "success";
  }
  if (/\bSCRIPT\b/.test(line.text)) {
    return "script";
  }
  return "output";
}

function subtitle(status: TestServerStatus | null, install: InstallState): string {
  if (!status) {
    return "Loading";
  }
  if (status.installing) {
    return install.progress ? `Installing, ${install.progress.phase} ${install.progress.percent.toFixed(0)}%` : "Installing";
  }
  if (status.runningHere) {
    return `Running on ${status.address}`;
  }
  if (status.running) {
    return "Another server folder is running";
  }
  return status.installed ? "Stopped" : "Not installed yet";
}

export function TestServerPanel({
  project,
  status,
  lines,
  install,
  tab,
  playing,
  onTabChange,
  onStatusChange,
  onInstall,
  onPlay,
  onStop,
  onClearConsole,
  onClose,
}: TestServerPanelProps) {
  const [command, setCommand] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [portDraft, setPortDraft] = useState<string | null>(null);
  const [mapDraft, setMapDraft] = useState<string | null>(null);
  const [settingDrafts, setSettingDrafts] = useState<Record<string, string>>({});
  const [copied, setCopied] = useState(false);
  const consoleRef = useRef<HTMLDivElement>(null);
  const floating = useFloating<HTMLElement>("test-server");
  const stickToBottomRef = useRef(true);

  useEffect(() => {
    const element = consoleRef.current;
    if (element && stickToBottomRef.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [lines, tab]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const save = async (patch: Partial<TestServerSettings>) => {
    if (!status) {
      return;
    }
    setError(null);
    try {
      onStatusChange(await api.updateTestServerSettings(project.projectFile, { ...status.settings, ...patch }));
    } catch (reason) {
      setError(toCommandError(reason).message);
    }
  };

  const gameModeFolder = status?.plan.gameMode ?? null;
  const gameModeSettings = project.packages.find((pkg) => pkg.folder === gameModeFolder)?.customSettings ?? [];

  const saveCustomSetting = async (key: string, value: boolean | number | string | undefined) => {
    const current = { ...(status?.settings.customSettings ?? {}) };
    if (value === undefined) {
      delete current[key];
    } else {
      current[key] = value;
    }
    await save({ customSettings: current });
  };

  const chooseFolder = async () => {
    const selected = await open({
      directory: true,
      defaultPath: status?.settings.directory ?? undefined,
      title: "Choose a folder for the test server, or an existing nanos world server",
    });
    if (typeof selected === "string") {
      await save({ directory: selected });
    }
  };

  const send = async () => {
    const line = command.trim();
    if (line === "") {
      return;
    }
    try {
      await api.sendTestServerCommand(line);
      setCommand("");
    } catch (reason) {
      setError(toCommandError(reason).message);
    }
  };

  const copyAddress = async () => {
    if (!status) {
      return;
    }
    try {
      await navigator.clipboard.writeText(status.address);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };

  const settings = status?.settings;
  const runningHere = Boolean(status?.runningHere);
  const busyInstalling = Boolean(status?.installing);

  return (
    <section ref={floating.ref} className={styles.panel} style={floating.style} aria-label="Test server" tabIndex={-1}>
      <header className={styles.header} {...floating.handle}>
        <span className={styles.statusIcon} data-state={runningHere ? "running" : busyInstalling ? "busy" : "idle"}>
          <Server size={16} />
        </span>
        <div className={styles.titles}>
          <h2>Test server</h2>
          <p>{subtitle(status, install)}</p>
        </div>
        {status?.installed && (
          <>
            <Button
              variant="primary"
              size="small"
              icon={runningHere ? <RotateCw size={14} /> : <Play size={14} />}
              busy={playing}
              disabled={busyInstalling}
              onClick={() => onPlay(false)}
              title={runningHere ? "Build and reload your packages (Alt+P)" : "Build and start the server (Alt+P)"}
            >
              {runningHere ? "Reload" : "Play"}
            </Button>
            {runningHere && (
              <>
                <Button variant="ghost" size="small" icon={<RotateCw size={14} />} disabled={playing} onClick={() => onPlay(true)}>
                  Restart
                </Button>
                <Button variant="ghost" size="small" icon={<Square size={13} />} onClick={onStop} title="Stop the server">
                  Stop
                </Button>
              </>
            )}
          </>
        )}
        <button type="button" className={styles.close} onClick={onClose} aria-label="Close the test server panel">
          <X size={15} />
        </button>
      </header>

      <div className={styles.tabs} role="tablist">
        {(["console", "setup"] as const).map((value) => (
          <button
            key={value}
            type="button"
            role="tab"
            className={styles.tab}
            aria-selected={tab === value}
            onClick={() => onTabChange(value)}
          >
            {value === "console" ? "Console" : "Setup"}
          </button>
        ))}
      </div>

      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}

      {tab === "console" ? (
        <>
          {status && (
            <div className={styles.connect}>
              <span>
                Join from nanos world with <strong>Find Servers</strong>, <strong>Quick Connect</strong> and
              </span>
              <code>{status.address}</code>
              <button type="button" className={styles.iconButton} onClick={() => void copyAddress()} title="Copy the address">
                <Copy size={13} />
              </button>
              {copied && <span className={styles.copied}>Copied</span>}
              <span className={styles.steamApp}>{settings?.steamApp === "game" ? "Main game" : "Playtest"}</span>
            </div>
          )}
          <div
            ref={consoleRef}
            className={styles.console}
            onScroll={(event) => {
              const element = event.currentTarget;
              stickToBottomRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < 24;
            }}
          >
            {lines.length === 0 ? (
              <p className={styles.empty}>
                {status?.installed
                  ? "Press Play to build your packages and start the server. Its console output appears here, runtime errors also show up on their nodes."
                  : "Install the test server in Setup first."}
              </p>
            ) : (
              lines.map((line) => (
                <div key={line.id} className={styles.line} data-tone={lineTone(line)}>
                  {line.text}
                </div>
              ))
            )}
          </div>
          <form
            className={styles.commandBar}
            onSubmit={(event) => {
              event.preventDefault();
              void send();
            }}
          >
            <input
              value={command}
              disabled={!runningHere}
              placeholder={runningHere ? "Server command, for example: package reload all" : "Start the server to send commands"}
              aria-label="Server console command"
              spellCheck={false}
              onChange={(event) => setCommand(event.target.value)}
            />
            <button type="button" className={styles.iconButton} onClick={onClearConsole} title="Clear the console">
              <Trash2 size={13} />
            </button>
          </form>
        </>
      ) : (
        status &&
        settings && (
          <div className={styles.setup}>
            <section className={styles.section}>
              <h3>Server</h3>
              <ChoiceGrid columns={2}>
                <ChoiceCard
                  title="Shared NanosFlow server"
                  description="One server for all your projects. Installed once into the NanosFlow data folder."
                  icon={<Server size={17} />}
                  selected={status.usesGlobal}
                  onSelect={() => void save({ directory: null })}
                />
                <ChoiceCard
                  title="Own folder"
                  description="A separate server for this project, or a nanos world server you already have."
                  icon={<HardDrive size={17} />}
                  tint="#34d399"
                  selected={!status.usesGlobal}
                  onSelect={() => void chooseFolder()}
                />
              </ChoiceGrid>
              <div className={styles.location}>
                <code title={status.directory}>{status.directory}</code>
                {!status.usesGlobal && (
                  <Button variant="ghost" size="small" icon={<FolderOpen size={14} />} onClick={() => void chooseFolder()}>
                    Change
                  </Button>
                )}
                {status.installed && (
                  <Button
                    variant="ghost"
                    size="small"
                    icon={<FolderSearch size={14} />}
                    onClick={() => void revealItemInDir(`${status.directory}/Packages`)}
                    title="Show the server folder"
                  />
                )}
              </div>
              <div className={styles.install}>
                <div className={styles.installText}>
                  <strong>{status.installed ? "Installed" : "Not installed"}</strong>
                  <span>
                    {status.installed
                      ? "Update to get the newest server version. Your packages are rebuilt on the next Play."
                      : "NanosFlow downloads the nanos world server (about 200 MB) with SteamCMD. SteamCMD is fetched automatically if it is missing."}
                  </span>
                </div>
                <select
                  className={styles.select}
                  value={settings.branch}
                  aria-label="Server version"
                  disabled={busyInstalling}
                  onChange={(event) => void save({ branch: event.target.value as ServerBranch })}
                >
                  <option value="public">Stable</option>
                  <option value="bleeding-edge">Bleeding edge</option>
                </select>
                <Button
                  variant={status.installed ? "secondary" : "primary"}
                  size="small"
                  icon={<Download size={14} />}
                  busy={busyInstalling}
                  disabled={runningHere}
                  onClick={onInstall}
                  title={runningHere ? "Stop the server before updating it" : undefined}
                >
                  {status.installed ? "Update" : "Install"}
                </Button>
              </div>
              {(busyInstalling || install.result) && (
                <div className={styles.progress}>
                  {busyInstalling && (
                    <div className={styles.bar}>
                      <span style={{ width: `${install.progress?.percent ?? 4}%` }} />
                    </div>
                  )}
                  <p data-tone={install.result ? (install.result.success ? "success" : "error") : undefined}>
                    {busyInstalling ? (install.line ?? "Starting SteamCMD") : install.result?.message}
                  </p>
                </div>
              )}
            </section>

            <section className={styles.section}>
              <h3>Play</h3>
              <div className={styles.row}>
                <span className={styles.label}>Steam app</span>
                <div className={styles.segmented} role="radiogroup" aria-label="Steam app">
                  {(["playtest", "game"] as SteamApp[]).map((app) => (
                    <button
                      key={app}
                      type="button"
                      role="radio"
                      aria-checked={settings.steamApp === app}
                      onClick={() => void save({ steamApp: app })}
                    >
                      {app === "playtest" ? "Playtest" : "Main game"}
                    </button>
                  ))}
                </div>
                <span className={styles.hint}>Your nanos world client must be the same app to join.</span>
              </div>
              {status.gameModes.length > 1 && (
                <div className={styles.row}>
                  <span className={styles.label}>Game mode</span>
                  <select
                    className={styles.select}
                    value={status.plan.gameMode ?? ""}
                    onChange={(event) => void save({ gameMode: event.target.value })}
                  >
                    {status.gameModes.map((mode) => (
                      <option key={mode} value={mode}>
                        {mode}
                      </option>
                    ))}
                  </select>
                  <span className={styles.hint}>nanos world runs one game mode at a time.</span>
                </div>
              )}
              <div className={styles.row}>
                <span className={styles.label}>Map</span>
                <input
                  className={styles.input}
                  value={mapDraft ?? settings.map ?? ""}
                  placeholder="default-blank-map"
                  disabled={Boolean(status.plan.projectMap)}
                  spellCheck={false}
                  onChange={(event) => setMapDraft(event.target.value)}
                  onBlur={() => {
                    if (mapDraft !== null) {
                      void save({ map: mapDraft }).then(() => setMapDraft(null));
                    }
                  }}
                />
                <span className={styles.hint}>
                  {status.plan.projectMap ? `Uses your map package ${status.plan.projectMap}.` : "Built-in or Vault map package name."}
                </span>
              </div>
              <div className={styles.row}>
                <span className={styles.label}>Port</span>
                <input
                  className={styles.input}
                  inputMode="numeric"
                  value={portDraft ?? String(settings.port)}
                  onChange={(event) => setPortDraft(event.target.value.replace(/[^0-9]/g, ""))}
                  onBlur={() => {
                    if (portDraft !== null) {
                      void save({ port: Number(portDraft) || settings.port }).then(() => setPortDraft(null));
                    }
                  }}
                />
                <span className={styles.hint}>The query port is the next one.</span>
              </div>
              <label className={styles.check}>
                <input
                  type="checkbox"
                  checked={settings.downloadMissing}
                  onChange={(event) => void save({ downloadMissing: event.target.checked })}
                />
                <span>Download missing packages, maps and assets from the Vault when the server starts</span>
              </label>
              <div className={styles.plan}>
                <span className={styles.label}>Starts with</span>
                <code>
                  {status.plan.gameMode ? `game mode ${status.plan.gameMode}` : "no game mode"}
                  {status.plan.packages.length > 0 ? `, packages ${status.plan.packages.join(", ")}` : ""}, map {status.plan.map}
                </code>
                {status.plan.ignoredGameModes.length > 0 && (
                  <span className={styles.hint}>Not loaded: {status.plan.ignoredGameModes.join(", ")} (only one game mode can run).</span>
                )}
              </div>
            </section>

            {gameModeSettings.length > 0 && (
              <section className={styles.section}>
                <h3>Custom settings</h3>
                {gameModeSettings.map((setting) => {
                  const override = settings.customSettings?.[setting.key];
                  const label = setting.label || setting.key;
                  const choose = (value: string) => void saveCustomSetting(setting.key, value === "" ? undefined : setting.type === "boolean" ? value === "true" : value);
                  return (
                    <div key={setting.key} className={styles.row}>
                      <span className={styles.label} title={setting.key}>
                        {label}
                      </span>
                      {setting.type === "boolean" || setting.type === "select" ? (
                        <select className={styles.select} value={override === undefined ? "" : String(override)} onChange={(event) => choose(event.target.value)}>
                          <option value="">Default ({String(setting.default)})</option>
                          {(setting.type === "boolean" ? ["true", "false"] : (setting.options ?? [])).map((option) => (
                            <option key={option} value={option}>
                              {option}
                            </option>
                          ))}
                        </select>
                      ) : (
                        <input
                          className={styles.input}
                          inputMode={setting.type === "text" ? undefined : "decimal"}
                          value={settingDrafts[setting.key] ?? (override === undefined ? "" : String(override))}
                          placeholder={`Default: ${String(setting.default)}`}
                          spellCheck={false}
                          onChange={(event) => setSettingDrafts((current) => ({ ...current, [setting.key]: event.target.value }))}
                          onBlur={() => {
                            const draft = settingDrafts[setting.key];
                            if (draft === undefined) {
                              return;
                            }
                            const trimmed = draft.trim();
                            const value =
                              trimmed === ""
                                ? undefined
                                : setting.type === "integer"
                                  ? Math.trunc(Number(trimmed))
                                  : setting.type === "floating"
                                    ? Number(trimmed)
                                    : draft;
                            void saveCustomSetting(setting.key, typeof value === "number" && Number.isNaN(value) ? undefined : value).then(() =>
                              setSettingDrafts(({ [setting.key]: _, ...rest }) => rest),
                            );
                          }}
                        />
                      )}
                    </div>
                  );
                })}
                <span className={styles.hint}>Empty means the default. NanosFlow passes these values with --custom_settings when the server starts, so restart it to apply a change.</span>
              </section>
            )}
          </div>
        )
      )}
    </section>
  );
}
