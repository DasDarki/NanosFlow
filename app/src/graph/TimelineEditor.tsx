import { ChartSpline, Plus, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Button } from "../components/Button";
import { DEFAULT_TIMELINE_LENGTH, LENGTH_PROPERTY, LOOPING_PROPERTY, TRACKS_PROPERTY, timelineLength, timelineTracks, type TimelineTrack } from "./dynamic";
import { generateUlid } from "./ulid";
import styles from "./TimelineEditor.module.css";

interface TimelineEditorProps {
  properties: Record<string, unknown>;
  onCommit: (patch: Record<string, unknown>) => void;
  onClose: () => void;
}

interface Range {
  low: number;
  high: number;
}

interface Drag {
  key: number;
  range: Range;
}

const WIDTH = 640;
const HEIGHT = 260;
const PADDING = 28;
const KEY_RADIUS = 6;

function valueRange(track: TimelineTrack | undefined): Range {
  const values = track?.keys.map(([, value]) => value) ?? [];
  if (values.length === 0) {
    return { low: 0, high: 1 };
  }
  const low = Math.min(...values);
  const high = Math.max(...values);
  if (low === high) {
    return { low: low - 1, high: high + 1 };
  }
  const margin = (high - low) * 0.15;
  return { low: low - margin, high: high + margin };
}

function rounded(value: number): number {
  return Math.round(value * 1000) / 1000;
}

export function TimelineEditor({ properties, onCommit, onClose }: TimelineEditorProps) {
  const [length, setLength] = useState(() => timelineLength(properties));
  const [lengthDraft, setLengthDraft] = useState<string | null>(null);
  const [looping, setLooping] = useState(properties[LOOPING_PROPERTY] === true);
  const [tracks, setTracks] = useState<TimelineTrack[]>(() => timelineTracks(properties));
  const [selectedTrack, setSelectedTrack] = useState<string | null>(() => timelineTracks(properties)[0]?.id ?? null);
  const [selectedKey, setSelectedKey] = useState<number | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const latest = useRef({ length, looping, tracks });
  latest.current = { length, looping, tracks };

  const track = tracks.find((candidate) => candidate.id === selectedTrack);
  const range = useMemo(() => drag?.range ?? valueRange(track), [drag, track]);

  const commit = (next: Partial<{ length: number; looping: boolean; tracks: TimelineTrack[] }> = {}) => {
    const merged = { ...latest.current, ...next };
    onCommit({
      [LENGTH_PROPERTY]: merged.length,
      [LOOPING_PROPERTY]: merged.looping ? true : undefined,
      [TRACKS_PROPERTY]: merged.tracks.map((entry) => ({ id: entry.id, name: entry.name, keys: entry.keys })),
    });
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const typing = event.target instanceof HTMLInputElement;
      if (event.key === "Escape") {
        onClose();
      } else if ((event.key === "Delete" || event.key === "Backspace") && !typing && selectedKey !== null && track) {
        event.preventDefault();
        removeKey(selectedKey);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const toX = (time: number) => PADDING + (time / length) * (WIDTH - PADDING * 2);
  const toY = (value: number) => HEIGHT - PADDING - ((value - range.low) / (range.high - range.low)) * (HEIGHT - PADDING * 2);
  const fromPointer = (event: { clientX: number; clientY: number }, fixed: Range) => {
    const box = svgRef.current?.getBoundingClientRect();
    if (!box) {
      return null;
    }
    const x = ((event.clientX - box.left) / box.width) * WIDTH;
    const y = ((event.clientY - box.top) / box.height) * HEIGHT;
    const time = Math.min(Math.max(((x - PADDING) / (WIDTH - PADDING * 2)) * length, 0), length);
    const value = fixed.low + ((HEIGHT - PADDING - y) / (HEIGHT - PADDING * 2)) * (fixed.high - fixed.low);
    return { time: rounded(time), value: rounded(value) };
  };

  const updateTrack = (id: string, change: (entry: TimelineTrack) => TimelineTrack) => {
    const next = tracks.map((entry) => (entry.id === id ? change(entry) : entry));
    setTracks(next);
    return next;
  };

  const setKeys = (keys: [number, number][], commitNow: boolean) => {
    if (!track) {
      return;
    }
    const next = updateTrack(track.id, (entry) => ({ ...entry, keys }));
    if (commitNow) {
      commit({ tracks: next });
    }
  };

  const removeKey = (index: number) => {
    if (!track) {
      return;
    }
    setSelectedKey(null);
    setKeys(track.keys.filter((_, position) => position !== index), true);
  };

  const addKey = (event: React.MouseEvent) => {
    if (!track) {
      return;
    }
    const point = fromPointer(event, range);
    if (!point) {
      return;
    }
    const keys = [...track.keys, [point.time, point.value] as [number, number]].sort((left, right) => left[0] - right[0]);
    setSelectedKey(keys.findIndex(([time, value]) => time === point.time && value === point.value));
    setKeys(keys, true);
  };

  const startDrag = (index: number) => (event: ReactPointerEvent) => {
    event.stopPropagation();
    (event.target as Element).setPointerCapture(event.pointerId);
    setSelectedKey(index);
    setDrag({ key: index, range });
  };

  const moveDrag = (event: ReactPointerEvent) => {
    if (!drag || !track) {
      return;
    }
    const point = fromPointer(event, drag.range);
    if (!point) {
      return;
    }
    const keys = track.keys.map((key, index) => (index === drag.key ? ([point.time, point.value] as [number, number]) : key));
    setKeys(keys, false);
  };

  const endDrag = () => {
    if (!drag || !track) {
      return;
    }
    const moved = track.keys[drag.key];
    const keys = [...track.keys].sort((left, right) => left[0] - right[0]);
    setSelectedKey(keys.indexOf(moved));
    setDrag(null);
    setKeys(keys, true);
  };

  const addTrack = () => {
    const names = ["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"];
    const name = names.find((candidate) => !tracks.some((entry) => entry.name === candidate)) ?? `Track ${tracks.length + 1}`;
    const created: TimelineTrack = { id: generateUlid().slice(-6).toLowerCase(), name, keys: [[0, 0], [length, 1]] };
    const next = [...tracks, created];
    setTracks(next);
    setSelectedTrack(created.id);
    setSelectedKey(null);
    commit({ tracks: next });
  };

  const removeTrack = (id: string) => {
    const next = tracks.filter((entry) => entry.id !== id);
    setTracks(next);
    if (selectedTrack === id) {
      setSelectedTrack(next[0]?.id ?? null);
      setSelectedKey(null);
    }
    commit({ tracks: next });
  };

  const applyLength = () => {
    if (lengthDraft === null) {
      return;
    }
    const parsed = Number(lengthDraft.replace(",", "."));
    setLengthDraft(null);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      return;
    }
    const next = rounded(parsed);
    const clamped = tracks.map((entry) => ({ ...entry, keys: entry.keys.map(([time, value]) => [Math.min(time, next), value] as [number, number]) }));
    setLength(next);
    setTracks(clamped);
    commit({ length: next, tracks: clamped });
  };

  const key = selectedKey !== null ? track?.keys[selectedKey] : undefined;
  const ticks = Array.from({ length: 5 }, (_, index) => (length / 4) * index);
  const path = track?.keys.map(([time, value], index) => `${index === 0 ? "M" : "L"} ${toX(time)} ${toY(value)}`).join(" ") ?? "";
  const extended =
    track && track.keys.length > 0
      ? `M ${toX(0)} ${toY(track.keys[0][1])} L ${toX(track.keys[0][0])} ${toY(track.keys[0][1])} ${path.replace(/^M/, "L")} L ${toX(length)} ${toY(track.keys[track.keys.length - 1][1])}`
      : "";

  return (
    <div className={styles.backdrop} onMouseDown={onClose}>
      <div className={styles.dialog} role="dialog" aria-label="Timeline" onMouseDown={(event) => event.stopPropagation()}>
        <header className={styles.header}>
          <ChartSpline size={18} className={styles.icon} />
          <h2>Timeline</h2>
          <label className={styles.length}>
            Length
            <input
              value={lengthDraft ?? String(length)}
              inputMode="decimal"
              aria-label="Length in seconds"
              onChange={(event) => setLengthDraft(event.target.value)}
              onBlur={applyLength}
              onKeyDown={(event) => event.key === "Enter" && applyLength()}
            />
            s
          </label>
          <label className={styles.loop}>
            <input
              type="checkbox"
              checked={looping}
              onChange={(event) => {
                setLooping(event.target.checked);
                commit({ looping: event.target.checked });
              }}
            />
            Loop
          </label>
          <span className={styles.spacer} />
          <Button variant="primary" size="small" onClick={onClose}>
            Done
          </Button>
          <button type="button" className={styles.close} onClick={onClose} aria-label="Close the timeline editor">
            <X size={15} />
          </button>
        </header>
        <div className={styles.body}>
          <aside className={styles.tracks}>
            <span className={styles.tracksTitle}>Tracks</span>
            {tracks.map((entry) => (
              <div key={entry.id} className={styles.track} data-selected={entry.id === selectedTrack} onClick={() => setSelectedTrack(entry.id)}>
                <input
                  value={entry.name}
                  aria-label="Track name"
                  spellCheck={false}
                  onFocus={() => setSelectedTrack(entry.id)}
                  onChange={(event) => updateTrack(entry.id, (current) => ({ ...current, name: event.target.value }))}
                  onBlur={() => commit()}
                />
                <button type="button" title="Remove this track" onClick={() => removeTrack(entry.id)}>
                  <Trash2 size={12} />
                </button>
              </div>
            ))}
            <button type="button" className={styles.addTrack} onClick={addTrack}>
              <Plus size={12} /> Add track
            </button>
            <p className={styles.hint}>Every track becomes a number output of the node. Update runs about 30 times per second.</p>
          </aside>
          <div className={styles.canvasArea}>
            {track ? (
              <>
                <svg
                  ref={svgRef}
                  className={styles.canvas}
                  viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
                  onDoubleClick={addKey}
                  onPointerMove={moveDrag}
                  onPointerUp={endDrag}
                  onMouseDown={() => setSelectedKey(null)}
                >
                  {ticks.map((time) => (
                    <g key={time}>
                      <line className={styles.grid} x1={toX(time)} x2={toX(time)} y1={PADDING / 2} y2={HEIGHT - PADDING} />
                      <text className={styles.label} x={toX(time)} y={HEIGHT - 8} textAnchor="middle">
                        {rounded(time)}s
                      </text>
                    </g>
                  ))}
                  {[range.low, (range.low + range.high) / 2, range.high].map((value) => (
                    <g key={value}>
                      <line className={styles.grid} x1={PADDING} x2={WIDTH - PADDING} y1={toY(value)} y2={toY(value)} />
                      <text className={styles.label} x={4} y={toY(value) - 3}>
                        {rounded(value)}
                      </text>
                    </g>
                  ))}
                  <path className={styles.curve} d={extended} />
                  {track.keys.map(([time, value], index) => (
                    <circle
                      key={index}
                      className={styles.key}
                      data-selected={index === selectedKey}
                      cx={toX(time)}
                      cy={toY(value)}
                      r={KEY_RADIUS}
                      onPointerDown={startDrag(index)}
                      onMouseDown={(event) => event.stopPropagation()}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        removeKey(index);
                      }}
                    />
                  ))}
                </svg>
                <div className={styles.inspector}>
                  {key && selectedKey !== null ? (
                    <>
                      <label>
                        Time
                        <input
                          type="number"
                          step="0.05"
                          min={0}
                          max={length}
                          value={key[0]}
                          onChange={(event) => {
                            const time = Math.min(Math.max(Number(event.target.value) || 0, 0), length);
                            setKeys(track.keys.map((entry, index) => (index === selectedKey ? [time, entry[1]] : entry)), false);
                          }}
                          onBlur={() => setKeys([...track.keys].sort((left, right) => left[0] - right[0]), true)}
                        />
                      </label>
                      <label>
                        Value
                        <input
                          type="number"
                          step="0.1"
                          value={key[1]}
                          onChange={(event) => {
                            const value = Number(event.target.value) || 0;
                            setKeys(track.keys.map((entry, index) => (index === selectedKey ? [entry[0], value] : entry)), false);
                          }}
                          onBlur={() => commit()}
                        />
                      </label>
                      <button type="button" className={styles.removeKey} onClick={() => removeKey(selectedKey)}>
                        <Trash2 size={12} /> Remove key
                      </button>
                    </>
                  ) : (
                    <span className={styles.hint}>Double-click to add a key, drag keys to move them, right-click or Delete removes one.</span>
                  )}
                </div>
              </>
            ) : (
              <p className={styles.empty}>Add a track to animate a value over {length || DEFAULT_TIMELINE_LENGTH} seconds.</p>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
