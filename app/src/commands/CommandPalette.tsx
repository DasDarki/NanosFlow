import { BookOpen, ChevronRight, Search } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { HELP_TOPICS } from "../help/topics";
import { useCommandRegistry, type Command, type CommandGroup } from "./registry";
import styles from "./CommandPalette.module.css";

interface CommandPaletteProps {
  onOpenHelp: (topic: string) => void;
  onClose: () => void;
}

const GROUP_ORDER: CommandGroup[] = ["Run", "Project", "Editor", "Panels", "Session", "Graphs", "App", "Help"];
const RESULT_LIMIT = 80;

function score(command: Command, terms: string[]): number {
  const title = command.title.toLowerCase();
  const haystack = `${title} ${command.group.toLowerCase()} ${(command.keywords ?? "").toLowerCase()} ${(command.detail ?? "").toLowerCase()}`;
  let total = 0;
  for (const term of terms) {
    if (title.startsWith(term)) {
      total += 8;
    } else if (title.includes(` ${term}`)) {
      total += 5;
    } else if (title.includes(term)) {
      total += 3;
    } else if (haystack.includes(term)) {
      total += 1;
    } else {
      return 0;
    }
  }
  return total;
}

export function CommandPalette({ onOpenHelp, onClose }: CommandPaletteProps) {
  const registry = useCommandRegistry();
  const [query, setQuery] = useState("");
  const [highlighted, setHighlighted] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const all = useMemo<Command[]>(() => {
    const help: Command[] = HELP_TOPICS.map((topic) => ({
      id: `help:${topic.id}`,
      title: `Help: ${topic.title}`,
      group: "Help",
      detail: topic.summary,
      keywords: topic.keywords,
      run: () => onOpenHelp(topic.id),
    }));
    return [...registry.commands(), ...help];
  }, [registry, registry.version, onOpenHelp]);

  const results = useMemo(() => {
    const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (terms.length === 0) {
      return [...all]
        .filter((command) => command.group !== "Graphs" && command.group !== "Help")
        .sort((left, right) => GROUP_ORDER.indexOf(left.group) - GROUP_ORDER.indexOf(right.group));
    }
    return all
      .map((command) => ({ command, score: score(command, terms) }))
      .filter((entry) => entry.score > 0)
      .sort((left, right) => right.score - left.score || left.command.title.length - right.command.title.length)
      .slice(0, RESULT_LIMIT)
      .map((entry) => entry.command);
  }, [all, query]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    setHighlighted(0);
  }, [query]);

  useLayoutEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-index="${highlighted}"]`)?.scrollIntoView({ block: "nearest" });
  }, [highlighted]);

  const run = (command: Command | undefined) => {
    if (!command) {
      return;
    }
    onClose();
    window.setTimeout(command.run, 0);
  };

  const grouped = query.trim() === "";

  return (
    <div className={styles.backdrop} onMouseDown={onClose}>
      <div className={styles.palette} role="dialog" aria-label="Command palette" onMouseDown={(event) => event.stopPropagation()}>
        <div className={styles.searchRow}>
          <Search size={16} />
          <input
            ref={inputRef}
            className={styles.input}
            value={query}
            placeholder="Run an action, open a graph or search the help"
            spellCheck={false}
            aria-label="Search commands"
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "ArrowDown") {
                event.preventDefault();
                setHighlighted((index) => Math.min(index + 1, results.length - 1));
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                setHighlighted((index) => Math.max(index - 1, 0));
              } else if (event.key === "Enter") {
                event.preventDefault();
                run(results[highlighted]);
              } else if (event.key === "Escape") {
                event.preventDefault();
                onClose();
              }
            }}
          />
        </div>
        <div className={styles.results} ref={listRef} role="listbox" aria-label="Commands">
          {results.length === 0 && <p className={styles.empty}>Nothing matches "{query}". Try other words, or search the help.</p>}
          {results.map((command, index) => {
            const showGroup = grouped ? index === 0 || results[index - 1].group !== command.group : false;
            return (
              <div key={command.id}>
                {showGroup && <div className={styles.groupTitle}>{command.group}</div>}
                <button
                  type="button"
                  role="option"
                  data-index={index}
                  aria-selected={index === highlighted}
                  className={styles.result}
                  tabIndex={-1}
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseMove={() => setHighlighted(index)}
                  onClick={() => run(command)}
                >
                  {command.group === "Help" ? <BookOpen size={14} className={styles.icon} /> : <ChevronRight size={14} className={styles.icon} />}
                  <span className={styles.texts}>
                    <span className={styles.title}>{command.title}</span>
                    {command.detail && <span className={styles.detail}>{command.detail}</span>}
                  </span>
                  {!grouped && <span className={styles.group}>{command.group}</span>}
                  {command.shortcut && <kbd className={styles.shortcut}>{command.shortcut}</kbd>}
                </button>
              </div>
            );
          })}
        </div>
        <div className={styles.footer}>
          <span>
            <kbd>↑</kbd>
            <kbd>↓</kbd> select <kbd>Enter</kbd> run <kbd>Esc</kbd> close
          </span>
          <span>Type a graph name to jump to it</span>
        </div>
      </div>
    </div>
  );
}
