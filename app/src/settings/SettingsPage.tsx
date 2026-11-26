import { ArrowLeft, Bot, Bug, Keyboard, LayoutGrid, Monitor } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api } from "../api";
import { SHORTCUT_GROUPS } from "../commands/shortcuts";
import { Button } from "../components/Button";
import { Segmented, ToggleRow } from "../components/Choice";
import { TextInput } from "../components/Field";
import helpStyles from "../help/HelpPage.module.css";
import pageStyles from "../pages/Page.module.css";
import type { DebuggerStatus, McpStatus, NodeListLayout } from "../types";
import { AgentSettings } from "./AgentSettings";
import { usePreferences } from "./preferences";

export type SettingsSection = "interface" | "editor" | "agents" | "debugging" | "shortcuts";

interface SettingsPageProps {
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
  mcp: McpStatus | null;
  projectFile: string | null;
  onOpenHelp: (topic: string) => void;
  onClose: () => void;
}

const SECTIONS: { id: SettingsSection; title: string; description: string; icon: ReactNode }[] = [
  { id: "interface", title: "Interface", description: "Size of the interface and how the title bar looks.", icon: <Monitor size={15} /> },
  { id: "editor", title: "Editor", description: "How the node list is sorted and what the graph editor shows.", icon: <LayoutGrid size={15} /> },
  { id: "agents", title: "AI agents", description: "Let AI agents work in NanosFlow and choose what they may do.", icon: <Bot size={15} /> },
  { id: "debugging", title: "Debugging", description: "The port breakpoints use to talk to NanosFlow.", icon: <Bug size={15} /> },
  { id: "shortcuts", title: "Keyboard shortcuts", description: "Every shortcut at a glance.", icon: <Keyboard size={15} /> },
];

const SCALES = ["90", "100", "110", "125"] as const;

function Row({ title, description, children }: { title: string; description: ReactNode; children: ReactNode }) {
  return (
    <div className={pageStyles.row}>
      <div className={pageStyles.rowText}>
        <span className={pageStyles.rowTitle}>{title}</span>
        <span className={pageStyles.rowDescription}>{description}</span>
      </div>
      {children}
    </div>
  );
}

function InterfaceSection() {
  const { preferences, update } = usePreferences();
  return (
    <div className={pageStyles.group}>
      <Row title="Interface size" description="Scales every text and control of NanosFlow.">
        <Segmented<string>
          label="Interface size"
          value={String(preferences.interfaceScale)}
          onChange={(value) => void update({ interfaceScale: Number(value) })}
          options={SCALES.map((scale) => ({ value: scale, label: `${scale}%` }))}
        />
      </Row>
      <ToggleRow
        title="Show button labels in the title bar"
        description="Turn it off for a compact title bar with icons only. Hover a button to see what it does."
        checked={preferences.buttonLabels}
        onChange={(buttonLabels) => void update({ buttonLabels })}
      />
    </div>
  );
}

function EditorSection() {
  const { preferences, update } = usePreferences();
  return (
    <div className={pageStyles.group}>
      <Row
        title="Node list"
        description={
          preferences.nodeList === "flat"
            ? "Flat: one list sorted by how well a node matches what you type."
            : "Categorized: grouped into flow control, math, your project and the nanos world API by class. Typing still searches everything."
        }
      >
        <Segmented<NodeListLayout>
          label="Node list layout"
          value={preferences.nodeList}
          onChange={(nodeList) => void update({ nodeList })}
          options={[
            { value: "flat", label: "Flat" },
            { value: "categorized", label: "Categorized" },
          ]}
        />
      </Row>
      <ToggleRow
        title="Show the minimap"
        description="A small overview of the whole graph in the corner of the editor."
        checked={preferences.minimap}
        onChange={(minimap) => void update({ minimap })}
      />
    </div>
  );
}

function DebuggingSection({ onOpenHelp }: { onOpenHelp: (topic: string) => void }) {
  const { preferences, update } = usePreferences();
  const [port, setPort] = useState(String(preferences.debuggerPort));
  const [status, setStatus] = useState<DebuggerStatus | null>(null);

  const refresh = useCallback(() => {
    void api
      .debuggerStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    setPort(String(preferences.debuggerPort));
    refresh();
  }, [preferences.debuggerPort, refresh]);

  const parsed = Number(port);
  const valid = Number.isInteger(parsed) && parsed >= 1024 && parsed <= 65535;

  return (
    <div className={pageStyles.group}>
      <Row
        title="Debugger port"
        description={
          status?.error ??
          (status?.listening ? `Listening on 127.0.0.1:${status.port}. Breakpoints in a debug build connect to this port.` : "Starting...")
        }
      >
        <TextInput mono inputMode="numeric" value={port} onValueChange={setPort} aria-label="Debugger port" style={{ width: 96 }} />
        <Button
          size="small"
          variant="secondary"
          disabled={!valid || parsed === preferences.debuggerPort}
          onClick={() => void update({ debuggerPort: parsed }).then(refresh)}
        >
          Save
        </Button>
      </Row>
      <p className={pageStyles.note}>
        The port is written into the build, so build or press Play again after you change it.{" "}
        <a
          href="#debugging"
          onClick={(event) => {
            event.preventDefault();
            onOpenHelp("debugging");
          }}
        >
          How breakpoints work
        </a>
      </p>
    </div>
  );
}

function ShortcutsSection() {
  return (
    <div className={helpStyles.article}>
      {SHORTCUT_GROUPS.map((group) => (
        <section key={group.title} className={helpStyles.shortcutGroup}>
          <h3>{group.title}</h3>
          <dl className={helpStyles.shortcutList}>
            {group.shortcuts.map((shortcut) => (
              <div key={shortcut.keys}>
                <dt>
                  <kbd>{shortcut.keys}</kbd>
                </dt>
                <dd>{shortcut.action}</dd>
              </div>
            ))}
          </dl>
        </section>
      ))}
    </div>
  );
}

export function SettingsPage({ section, onSectionChange, mcp, projectFile, onOpenHelp, onClose }: SettingsPageProps) {
  const current = SECTIONS.find((entry) => entry.id === section) ?? SECTIONS[0];

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className={pageStyles.page} role="dialog" aria-label="Settings">
      <nav className={pageStyles.sidebar}>
        <button type="button" className={pageStyles.back} onClick={onClose}>
          <ArrowLeft size={15} />
          Back
        </button>
        <span className={pageStyles.sidebarTitle}>Settings</span>
        {SECTIONS.map((entry) => (
          <button
            key={entry.id}
            type="button"
            className={pageStyles.navItem}
            aria-current={entry.id === current.id ? "page" : undefined}
            onClick={() => onSectionChange(entry.id)}
          >
            {entry.icon}
            {entry.title}
          </button>
        ))}
      </nav>
      <div className={pageStyles.content}>
        <div className={pageStyles.inner}>
          <header className={pageStyles.heading}>
            <h1>{current.title}</h1>
            <p>{current.description} These settings apply to NanosFlow on this computer.</p>
          </header>
          {current.id === "interface" && <InterfaceSection />}
          {current.id === "editor" && <EditorSection />}
          {current.id === "agents" && <AgentSettings status={mcp} projectFile={projectFile} />}
          {current.id === "debugging" && <DebuggingSection onOpenHelp={onOpenHelp} />}
          {current.id === "shortcuts" && <ShortcutsSection />}
        </div>
      </div>
    </div>
  );
}
