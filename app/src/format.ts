const relativeFormatter = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 60 * 60 * 24 * 365],
  ["month", 60 * 60 * 24 * 30],
  ["week", 60 * 60 * 24 * 7],
  ["day", 60 * 60 * 24],
  ["hour", 60 * 60],
  ["minute", 60],
];

export function formatRelativeTime(unixSeconds: number): string {
  const elapsed = unixSeconds - Date.now() / 1000;
  for (const [unit, seconds] of UNITS) {
    if (Math.abs(elapsed) >= seconds) {
      return relativeFormatter.format(Math.round(elapsed / seconds), unit);
    }
  }
  return "just now";
}

export function joinPath(base: string, ...segments: string[]): string {
  const separator = base.includes("\\") && !base.includes("/") ? "\\" : "/";
  const trimmed = base.replace(/[\\/]+$/, "");
  return [trimmed, ...segments.filter(Boolean)].join(separator);
}

export function parentDirectory(path: string): string {
  const index = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return index > 0 ? path.slice(0, index) : path;
}

export const isMac = navigator.userAgent.includes("Mac");
export const modifierLabel = isMac ? "⌘" : "Ctrl";
