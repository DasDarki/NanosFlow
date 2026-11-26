import { Check, ChevronDown, Copy, FolderCog, Globe } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { Segmented, ToggleRow } from "../components/Choice";
import { TextInput } from "../components/Field";
import { useToasts } from "../components/Toasts";
import type { McpClientSetup, McpInstallScope, McpPermissions, McpSettings, McpStatus } from "../types";
import styles from "./AgentSettings.module.css";

interface AgentSettingsProps {
  status: McpStatus | null;
  projectFile: string | null;
}

const DEFAULT_PERMISSIONS: McpPermissions = {
  editGraphs: true,
  changeProject: true,
  build: true,
  testServer: true,
  navigate: true,
  showCursor: true,
};

const PERMISSIONS: { key: keyof McpPermissions; title: string; description: string }[] = [
  { key: "editGraphs", title: "Edit graphs", description: "Add, connect, move and remove nodes." },
  { key: "changeProject", title: "Change the project", description: "Create graphs and change variables and signatures." },
  { key: "build", title: "Build and preview Lua", description: "Compile the project and read the generated code." },
  { key: "testServer", title: "Use the test server", description: "Start and stop the server and read its console." },
  { key: "navigate", title: "Open graphs in the editor", description: "Switch the graph you are looking at to show you something." },
  { key: "showCursor", title: "Show the agent's cursor", description: "Mark where an agent works with a cursor and highlighted nodes." },
];

function ago(timestamp: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - timestamp) / 1000));
  if (seconds < 5) {
    return "just now";
  }
  if (seconds < 60) {
    return `${seconds} s ago`;
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return `${minutes} min ago`;
  }
  return new Date(timestamp).toLocaleString();
}

function CopyBlock({ text, language }: { text: string; language: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div className={styles.codeBlock} data-language={language}>
      <pre>{text}</pre>
      <button type="button" className={styles.copy} onClick={() => void copy()} aria-label="Copy" title="Copy">
        {copied ? <Check size={14} /> : <Copy size={14} />}
      </button>
    </div>
  );
}

function stateLabel(setup: McpClientSetup): { text: string; tone: string } | null {
  if (setup.state === "configured") {
    return { text: "Set up", tone: "success" };
  }
  if (setup.state === "outdated") {
    return { text: "Other port", tone: "warning" };
  }
  return setup.detected ? { text: "Installed", tone: "muted" } : null;
}

function ClientCard({
  setup,
  scope,
  onScopeChange,
  projectFile,
  onInstalled,
}: {
  setup: McpClientSetup;
  scope: McpInstallScope;
  onScopeChange: (scope: McpInstallScope) => void;
  projectFile: string | null;
  onInstalled: () => void;
}) {
  const { notify, reportError } = useToasts();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const label = stateLabel(setup);
  const projectScope = setup.supportsProjectScope && scope === "project";

  const install = async () => {
    setBusy(true);
    try {
      const message = await api.installMcpClient(setup.kind, setup.supportsProjectScope ? scope : "user", projectFile);
      notify({ kind: "success", title: `${setup.name} is connected`, message });
      onInstalled();
    } catch (error) {
      reportError(toCommandError(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className={styles.client} data-open={open}>
      <div className={styles.clientRow}>
        <span className={styles.clientName}>{setup.name}</span>
        {label && (
          <span className={styles.badge} data-tone={label.tone}>
            {label.tone === "success" && <Check size={11} />}
            {label.text}
          </span>
        )}
        <span className={styles.spacer} />
        <Button
          variant={setup.state === "configured" ? "ghost" : "primary"}
          size="small"
          busy={busy}
          disabled={projectScope && !projectFile}
          title={projectScope && !projectFile ? "Open a project to add NanosFlow to its .mcp.json" : undefined}
          onClick={() => void install()}
        >
          {setup.state === "configured" ? "Set up again" : setup.oneClick}
        </Button>
        <button
          type="button"
          className={styles.expand}
          onClick={() => setOpen((value) => !value)}
          aria-expanded={open}
          aria-label={`Manual setup for ${setup.name}`}
          title="Set it up yourself"
        >
          <ChevronDown size={15} />
        </button>
      </div>
      {setup.supportsProjectScope && (
        <div className={styles.scope}>
          <Segmented<McpInstallScope>
            label={`Where ${setup.name} uses NanosFlow`}
            value={scope}
            onChange={onScopeChange}
            options={[
              { value: "user", label: "All folders", icon: <Globe size={13} /> },
              { value: "project", label: "This project", icon: <FolderCog size={13} /> },
            ]}
          />
          <span className={styles.scopeHint}>
            {scope === "user"
              ? "User scope: available in every Claude Code session."
              : "Project scope: writes .mcp.json into the project, so everyone who clones it gets the server."}
          </span>
        </div>
      )}
      {open && (
        <div className={styles.manual}>
          {setup.snippets.map((snippet) => (
            <div key={snippet.label} className={styles.snippet}>
              <span className={styles.snippetLabel}>{snippet.label}</span>
              <CopyBlock text={snippet.text} language={snippet.language} />
            </div>
          ))}
        </div>
      )}
    </li>
  );
}

export function AgentSettings({ status, projectFile }: AgentSettingsProps) {
  const { reportError } = useToasts();
  const [scope, setScope] = useState<McpInstallScope>("user");
  const [setups, setSetups] = useState<McpClientSetup[]>([]);
  const [port, setPort] = useState(String(status?.settings.port ?? 7920));
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 5000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (status) {
      setPort(String(status.settings.port));
    }
  }, [status?.settings.port]);

  const refreshSetups = useCallback(() => {
    void api
      .mcpClientSetups(scope, projectFile)
      .then(setSetups)
      .catch(() => undefined);
  }, [scope, projectFile]);

  useEffect(refreshSetups, [refreshSetups, status?.settings.port]);

  const save = async (settings: McpSettings) => {
    try {
      await api.updateMcpSettings(settings);
    } catch (error) {
      reportError(toCommandError(error));
    }
  };

  const settings: McpSettings = status?.settings ?? { enabled: false, port: 7920, permissions: DEFAULT_PERMISSIONS };
  const permissions = settings.permissions ?? DEFAULT_PERMISSIONS;
  const parsedPort = Number(port);
  const portValid = Number.isInteger(parsedPort) && parsedPort >= 1024 && parsedPort <= 65535;
  const setPermission = (key: keyof McpPermissions, value: boolean) => void save({ ...settings, permissions: { ...permissions, [key]: value } });

  return (
    <>
      <section className={styles.section}>
        <ToggleRow
          title="Let AI agents control NanosFlow"
          description="Starts a local MCP server. Agents like Claude Code can then read and edit your graphs, build the project and use the test server, live in this window."
          checked={settings.enabled}
          onChange={(checked) => void save({ ...settings, enabled: checked })}
        />
        {status && (
          <div className={styles.server} data-state={status.error ? "error" : status.running ? "running" : "off"}>
            <span className={styles.dot} />
            <span className={styles.serverText}>
              {status.error ? status.error : status.running ? "Listening on" : "Off. Setting up a client below turns it on."}
              {status.running && !status.error && <code>{status.url}</code>}
            </span>
            <label className={styles.port}>
              Port
              <TextInput mono inputMode="numeric" value={port} onValueChange={setPort} aria-label="MCP port" />
            </label>
            <Button size="small" variant="secondary" disabled={!portValid || parsedPort === settings.port} onClick={() => void save({ ...settings, port: parsedPort })}>
              Save
            </Button>
          </div>
        )}
        <p className={styles.note}>Only programs on this computer can connect. Websites are refused. Each change an agent makes is one step you can undo with Ctrl+Z.</p>
      </section>

      <section className={styles.section}>
        <h3>What agents may do</h3>
        <div className={styles.permissions}>
          {PERMISSIONS.map((permission) => (
            <ToggleRow
              key={permission.key}
              title={permission.title}
              description={permission.description}
              checked={permissions[permission.key]}
              onChange={(checked) => setPermission(permission.key, checked)}
            />
          ))}
        </div>
        <p className={styles.note}>Changed abilities apply to new agent sessions. Restart the agent, or reconnect its MCP server, to pick them up.</p>
      </section>

      <section className={styles.section}>
        <h3>Set up a client</h3>
        <ul className={styles.clients}>
          {setups.map((setup) => (
            <ClientCard key={setup.kind} setup={setup} scope={scope} onScopeChange={setScope} projectFile={projectFile} onInstalled={refreshSetups} />
          ))}
        </ul>
      </section>

      {status && (status.clients.length > 0 || status.activity.length > 0) && (
        <section className={styles.section}>
          <h3>Recent activity</h3>
          <ul className={styles.connected}>
            {status.clients.map((client) => (
              <li key={client.name}>
                <span className={styles.dot} data-recent={now - client.lastSeen < 15000} />
                <span className={styles.clientName}>{client.name}</span>
                <span className={styles.muted}>{ago(client.lastSeen, now)}</span>
              </li>
            ))}
          </ul>
          <ol className={styles.activity}>
            {status.activity
              .slice(-12)
              .reverse()
              .map((entry) => (
                <li key={entry.id}>
                  <span className={styles.muted}>{entry.client}</span> {entry.summary}
                </li>
              ))}
          </ol>
        </section>
      )}
    </>
  );
}
