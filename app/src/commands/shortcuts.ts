import { modifierLabel } from "../format";

export interface Shortcut {
  keys: string;
  action: string;
}

export interface ShortcutGroup {
  title: string;
  shortcuts: Shortcut[];
}

const mod = modifierLabel;

export const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    title: "Everywhere",
    shortcuts: [
      { keys: `${mod} Shift P`, action: "Command palette: search actions, graphs and help" },
      { keys: "F1", action: "Open the help" },
      { keys: `${mod} ,`, action: "Open the settings" },
      { keys: `${mod} N`, action: "New project" },
      { keys: `${mod} O`, action: "Open a project" },
    ],
  },
  {
    title: "Project",
    shortcuts: [
      { keys: `${mod} B`, action: "Build the project" },
      { keys: "Alt P", action: "Play on the test server, or join the host's game in a session" },
      { keys: `${mod} Shift F`, action: "Search the whole project" },
    ],
  },
  {
    title: "Graph editor",
    shortcuts: [
      { keys: "Space", action: "Add a node at the mouse" },
      { keys: "Right drag", action: "Pan the canvas" },
      { keys: "Left drag", action: "Select nodes" },
      { keys: "Alt click pin", action: "Disconnect a pin" },
      { keys: "Double-click wire", action: "Add a reroute point" },
      { keys: "C", action: "Comment around the selection" },
      { keys: "F", action: "Fit the selection or the whole graph" },
      { keys: "F9", action: "Toggle a breakpoint on the selected node" },
      { keys: `${mod} D`, action: "Duplicate" },
      { keys: `${mod} C / X / V`, action: "Copy, cut, paste" },
      { keys: `${mod} A`, action: "Select everything" },
      { keys: `${mod} Z / Y`, action: "Undo, redo" },
      { keys: `${mod} S`, action: "Save now (changes also save on their own)" },
    ],
  },
];
