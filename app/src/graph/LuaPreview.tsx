import { Check, CircleAlert, Copy, FileCode, LoaderCircle, TriangleAlert } from "lucide-react";
import { useMemo, useState } from "react";
import type { BuildProblem } from "../types";
import type { LuaPreviewState } from "./GraphEditor";
import styles from "./LuaPreview.module.css";

interface LuaPreviewProps {
  state: LuaPreviewState;
  graphKind: string;
  onSelectProblem: (problem: BuildProblem) => void;
}

type TokenKind = "comment" | "string" | "number" | "keyword" | "constant" | "call" | "plain";

const KEYWORDS = new Set([
  "and",
  "break",
  "do",
  "else",
  "elseif",
  "end",
  "for",
  "function",
  "goto",
  "if",
  "in",
  "local",
  "not",
  "or",
  "repeat",
  "return",
  "then",
  "until",
  "while",
]);
const CONSTANTS = new Set(["true", "false", "nil", "self"]);
const TOKEN_PATTERN = /(--.*$)|("(?:[^"\\]|\\.)*")|(\b\d+(?:\.\d+)?\b)|([A-Za-z_][A-Za-z0-9_]*)/g;

function tokenize(line: string): { kind: TokenKind; text: string }[] {
  const tokens: { kind: TokenKind; text: string }[] = [];
  let last = 0;
  for (const match of line.matchAll(TOKEN_PATTERN)) {
    const index = match.index ?? 0;
    if (index > last) {
      tokens.push({ kind: "plain", text: line.slice(last, index) });
    }
    const [text, comment, string, number, word] = match;
    let kind: TokenKind = "plain";
    if (comment) {
      kind = "comment";
    } else if (string) {
      kind = "string";
    } else if (number) {
      kind = "number";
    } else if (word && KEYWORDS.has(word)) {
      kind = "keyword";
    } else if (word && CONSTANTS.has(word)) {
      kind = "constant";
    } else if (word && /^\s*\(/.test(line.slice(index + text.length))) {
      kind = "call";
    }
    tokens.push({ kind, text });
    last = index + text.length;
  }
  if (last < line.length) {
    tokens.push({ kind: "plain", text: line.slice(last) });
  }
  return tokens;
}

function emptyMessage(graphKind: string): string {
  if (graphKind === "event-graph") {
    return "This graph does not produce any Lua yet. Events only turn into Lua once something is connected to them.";
  }
  if (graphKind === "macro") {
    return "A macro has no Lua file of its own. Its nodes are copied into every graph that uses it, open one of those graphs to see the result.";
  }
  return "No Lua was produced for this graph.";
}

export function LuaPreview({ state, graphKind, onSelectProblem }: LuaPreviewProps) {
  const { data: preview, error, loading } = state;
  const [copied, setCopied] = useState(false);

  const lines = useMemo(() => (preview?.code ? preview.code.replace(/\n$/, "").split("\n") : []), [preview]);
  const problems = preview?.problems ?? [];
  const errors = problems.filter((problem) => problem.severity === "error").length;

  const copy = () => {
    if (!preview?.code) {
      return;
    }
    void navigator.clipboard.writeText(preview.code).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    });
  };

  return (
    <section className={styles.preview} aria-label="Generated Lua">
      <header className={styles.header}>
        <FileCode size={14} />
        <code className={styles.file}>{preview?.file ?? "Lua output"}</code>
        {loading && <LoaderCircle size={13} className={styles.spin} aria-label="Updating" />}
        <button type="button" className={styles.copy} onClick={copy} disabled={!preview?.code} title="Copy the Lua code">
          {copied ? <Check size={13} /> : <Copy size={13} />}
          {copied ? "Copied" : "Copy"}
        </button>
      </header>

      {error ? (
        <div className={styles.message} data-severity="error">
          <CircleAlert size={15} />
          <div>
            <strong>{error.title}</strong>
            <p>{error.message}</p>
          </div>
        </div>
      ) : (
        <>
          {errors > 0 && (
            <p className={styles.notice}>
              The build stops at {errors === 1 ? "this error" : `these ${errors} errors`}. The code below shows what NanosFlow would
              write once {errors === 1 ? "it is" : "they are"} fixed.
            </p>
          )}
          {problems.length > 0 && (
            <div className={styles.problems}>
              {problems.map((problem, index) => (
                <button
                  key={`${problem.code}-${problem.node ?? ""}-${index}`}
                  type="button"
                  className={styles.problem}
                  data-severity={problem.severity}
                  onClick={() => onSelectProblem(problem)}
                  disabled={!problem.node}
                  title={problem.node ? "Show the node" : undefined}
                >
                  {problem.severity === "error" ? <CircleAlert size={14} /> : <TriangleAlert size={14} />}
                  <span>
                    {problem.message}
                    {problem.hint && <em>{problem.hint}</em>}
                  </span>
                </button>
              ))}
            </div>
          )}
          {lines.length > 0 ? (
            <pre className={styles.code}>
              {lines.map((line, index) => (
                <div key={index} className={styles.line}>
                  <span className={styles.number}>{index + 1}</span>
                  <span className={styles.text}>
                    {tokenize(line).map((token, tokenIndex) =>
                      token.kind === "plain" ? (
                        token.text
                      ) : (
                        <span key={tokenIndex} data-token={token.kind}>
                          {token.text}
                        </span>
                      ),
                    )}
                  </span>
                </div>
              ))}
            </pre>
          ) : (
            !loading && <p className={styles.empty}>{emptyMessage(graphKind)}</p>
          )}
        </>
      )}
    </section>
  );
}
