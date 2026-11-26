import { Check, Copy, Gamepad2, LogIn, LogOut, Play, Radio, Settings2, UserMinus, Users, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, toCommandError } from "../api";
import { Button } from "../components/Button";
import { Field, TextInput } from "../components/Field";
import { useToasts } from "../components/Toasts";
import { useFloating } from "../components/useFloating";
import type { SessionSettings, SessionStatus, SessionView } from "../types";
import type { SessionApi } from "./useSession";
import styles from "./SessionPanel.module.css";

interface SessionPanelProps {
  session: SessionApi;
  projectFile: string | null;
  onClose: () => void;
}

function subtitle(view: SessionView): string {
  switch (view.type) {
    case "idle":
      return "Play together through your relay";
    case "starting":
      return "Connecting to the relay";
    case "waiting":
      return `Waiting for the host of ${view.code}`;
    case "hosting":
      return `Hosting ${view.code}, ${view.guests.length} ${view.guests.length === 1 ? "guest" : "guests"}`;
    case "joined":
      return `In the session of ${view.hostName}`;
  }
}

export function sessionState(status: SessionStatus | null): "idle" | "busy" | "live" {
  const type = status?.view.type ?? "idle";
  if (type === "hosting" || type === "joined") {
    return "live";
  }
  return type === "idle" ? "idle" : "busy";
}

function CopyButton({ text, label }: { text: string; label: string }) {
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
    <button type="button" className={styles.iconButton} onClick={() => void copy()} title={label} aria-label={label}>
      {copied ? <Check size={14} /> : <Copy size={14} />}
    </button>
  );
}

function SettingsForm({ settings, session, open, onToggle }: { settings: SessionSettings; session: SessionApi; open: boolean; onToggle: () => void }) {
  const [draft, setDraft] = useState(settings);
  const [port, setPort] = useState(String(settings.guestPort));
  useEffect(() => {
    setDraft(settings);
    setPort(String(settings.guestPort));
  }, [settings]);
  const parsedPort = Number(port);
  const dirty = draft.relay !== settings.relay || draft.name !== settings.name || parsedPort !== settings.guestPort;

  return (
    <section className={styles.section}>
      <button type="button" className={styles.sectionToggle} onClick={onToggle} aria-expanded={open}>
        <Settings2 size={14} />
        Connection settings
        {!settings.relay && <span className={styles.badge}>Needed</span>}
      </button>
      {open && (
        <form
          className={styles.settings}
          onSubmit={(event) => {
            event.preventDefault();
            void session.run(() => api.updateSessionSettings({ ...draft, guestPort: parsedPort }));
          }}
        >
          <Field label="Relay" htmlFor="session-relay" hint="The address nf-relay prints when it starts, including the part after #.">
            <TextInput
              id="session-relay"
              mono
              value={draft.relay}
              placeholder="relay.example.com:7900#fingerprint"
              onValueChange={(relay) => setDraft({ ...draft, relay })}
            />
          </Field>
          <div className={styles.row}>
            <Field label="Your name" htmlFor="session-name" optional>
              <TextInput id="session-name" value={draft.name} maxLength={32} placeholder="Shown to the host" onValueChange={(name) => setDraft({ ...draft, name })} />
            </Field>
            <Field label="Local game port" htmlFor="session-port" hint="Used when you join.">
              <TextInput id="session-port" mono inputMode="numeric" value={port} onValueChange={setPort} />
            </Field>
          </div>
          <div className={styles.actions}>
            <Button type="submit" variant="secondary" size="small" disabled={!dirty || !Number.isInteger(parsedPort)}>
              Save
            </Button>
          </div>
        </form>
      )}
    </section>
  );
}

export function SessionPanel({ session, projectFile, onClose }: SessionPanelProps) {
  const { status, run } = session;
  const { notify, reportError } = useToasts();
  const joinGame = async () => {
    try {
      notify({ kind: "success", title: "Joining the game", message: await api.joinSessionGame() });
    } catch (error) {
      reportError(toCommandError(error));
    }
  };
  const [code, setCode] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const view = status?.view ?? { type: "idle" as const };
  const relayMissing = status !== null && !status.settings.relay;

  useEffect(() => {
    if (relayMissing) {
      setSettingsOpen(true);
    }
  }, [relayMissing]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const leave = () => void run(api.leaveSession);
  const floating = useFloating<HTMLElement>("session");

  return (
    <section ref={floating.ref} className={styles.panel} style={floating.style} aria-label="Session">
      <header className={styles.header} {...floating.handle}>
        <span className={styles.statusIcon} data-state={sessionState(status)}>
          <Users size={16} />
        </span>
        <div className={styles.titles}>
          <h2>Session</h2>
          <p>{subtitle(view)}</p>
        </div>
        {view.type === "hosting" && (
          <Button variant="ghost" size="small" icon={<LogOut size={14} />} onClick={leave}>
            End session
          </Button>
        )}
        {view.type === "joined" && (
          <Button variant="ghost" size="small" icon={<LogOut size={14} />} onClick={leave}>
            Leave
          </Button>
        )}
        <button type="button" className={styles.iconButton} onClick={onClose} aria-label="Close the session panel">
          <X size={15} />
        </button>
      </header>

      <div className={styles.body}>
        {view.type === "idle" && status && (
          <>
            <div className={styles.cards}>
              <div className={styles.card}>
                <h3>
                  <Radio size={15} />
                  Host
                </h3>
                <p>Open a session for this project. Admitted guests play on your test server through the relay.</p>
                <Button
                  variant="primary"
                  size="small"
                  disabled={!projectFile || relayMissing}
                  title={projectFile ? undefined : "Open a project to host a session"}
                  onClick={() => projectFile && void run(() => api.hostSession(projectFile))}
                >
                  Host session
                </Button>
              </div>
              <form
                className={styles.card}
                onSubmit={(event) => {
                  event.preventDefault();
                  void run(() => api.joinSession(code)).then((joined) => joined && setCode(""));
                }}
              >
                <h3>
                  <LogIn size={15} />
                  Join
                </h3>
                <p>Enter the code the host gave you.</p>
                <div className={styles.joinRow}>
                  <TextInput mono value={code} maxLength={8} placeholder="K7Q-M2X" aria-label="Session code" onValueChange={(value) => setCode(value.toUpperCase())} />
                  <Button type="submit" variant="primary" size="small" disabled={relayMissing || code.replace(/[^a-z0-9]/gi, "").length !== 6}>
                    Join
                  </Button>
                </div>
              </form>
            </div>
            <SettingsForm settings={status.settings} session={session} open={settingsOpen} onToggle={() => setSettingsOpen((value) => !value)} />
          </>
        )}

        {(view.type === "starting" || view.type === "waiting") && (
          <div className={styles.waiting}>
            <span className={styles.pulse} />
            <p>{view.type === "starting" ? "Opening your session on the relay..." : "The host has been asked to let you in. This can take a moment."}</p>
            <Button variant="secondary" size="small" onClick={leave}>
              Cancel
            </Button>
          </div>
        )}

        {view.type === "hosting" && (
          <>
            <div className={styles.codeBox}>
              <span className={styles.codeLabel}>Session code</span>
              <code className={styles.code}>{view.code}</code>
              <CopyButton text={view.code} label="Copy the session code" />
            </div>
            <div className={styles.gameState} data-running={view.gameRunning}>
              <Gamepad2 size={15} />
              <span>
                {view.gameRunning ? (
                  <>
                    Your test server runs. Guests can press <strong>Join game</strong> to play on <code>{view.server}</code>.
                  </>
                ) : (
                  <>
                    Press <strong>Play</strong> to start your test server, then guests can join the game.
                  </>
                )}
              </span>
            </div>
            {view.requests.length > 0 && (
              <section className={styles.section}>
                <h3 className={styles.sectionTitle}>Wants to join</h3>
                <ul className={styles.people}>
                  {view.requests.map((person) => (
                    <li key={person.id} data-request="true">
                      <span className={styles.avatar}>{person.name.slice(0, 1).toUpperCase()}</span>
                      <span className={styles.personName}>{person.name}</span>
                      <Button variant="ghost" size="small" onClick={() => void run(() => api.answerJoinRequest(person.id, false))}>
                        Decline
                      </Button>
                      <Button variant="primary" size="small" onClick={() => void run(() => api.answerJoinRequest(person.id, true))}>
                        Admit
                      </Button>
                    </li>
                  ))}
                </ul>
              </section>
            )}
            <section className={styles.section}>
              <h3 className={styles.sectionTitle}>Guests</h3>
              {view.guests.length === 0 ? (
                <p className={styles.empty}>Nobody joined yet.</p>
              ) : (
                <ul className={styles.people}>
                  {view.guests.map((person) => (
                    <li key={person.id}>
                      <span className={styles.avatar}>{person.name.slice(0, 1).toUpperCase()}</span>
                      <span className={styles.personName}>{person.name}</span>
                      <button
                        type="button"
                        className={styles.iconButton}
                        onClick={() => void run(() => api.kickGuest(person.id))}
                        title={`Remove ${person.name} from the session`}
                        aria-label={`Remove ${person.name}`}
                      >
                        <UserMinus size={14} />
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </>
        )}

        {view.type === "joined" && (
          <>
            <div className={styles.gameState} data-running={view.game.running}>
              <Gamepad2 size={15} />
              <span>{view.game.running ? `${view.hostName} runs the test server.` : `Waiting for ${view.hostName} to start the test server.`}</span>
              <Button variant="primary" size="small" icon={<Play size={13} />} disabled={!view.game.running} onClick={() => void joinGame()}>
                Join game
              </Button>
            </div>
            <div className={styles.codeBox}>
              <span className={styles.codeLabel}>Direct Connect to</span>
              <code className={styles.code}>{view.address}</code>
              <CopyButton text={view.address} label="Copy the game address" />
            </div>
            <p className={styles.hint}>
              Join game starts nanos world through Steam and connects it for you. You can also open <strong>Direct Connect</strong> in nanos world
              and enter this address yourself.
            </p>
          </>
        )}

        {status && status.activity.length > 0 && (
          <section className={styles.section}>
            <h3 className={styles.sectionTitle}>Activity</h3>
            <ol className={styles.activity}>
              {status.activity
                .slice(-40)
                .reverse()
                .map((entry) => (
                  <li key={entry.id} data-tone={entry.tone}>
                    {entry.text}
                  </li>
                ))}
            </ol>
          </section>
        )}
      </div>
    </section>
  );
}
