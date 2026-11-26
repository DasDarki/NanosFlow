import type { ReactNode } from "react";
import { SHORTCUT_GROUPS } from "../commands/shortcuts";
import styles from "./HelpPage.module.css";

export interface HelpTopic {
  id: string;
  title: string;
  summary: string;
  keywords: string;
  body: ReactNode;
}

function Keys({ children }: { children: string }) {
  return <kbd>{children}</kbd>;
}

export const HELP_TOPICS: HelpTopic[] = [
  {
    id: "getting-started",
    title: "Getting started",
    summary: "Projects, packages and graphs, and how they become a nanos world package.",
    keywords: "start begin project package graph new create basics",
    body: (
      <>
        <p>
          A <strong>project</strong> is a folder with a <code>.nfproj</code> file. It holds one or more <strong>packages</strong>, usually one game mode and
          maybe a few scripts. Every package is split into <strong>server</strong>, <strong>client</strong> and <strong>shared</strong> scripts, and each of
          them contains <strong>graphs</strong>.
        </p>
        <p>
          A graph is a network of nodes. Events like <em>Package Load</em> or <em>Player Spawn</em> start the flow, white exec wires decide the order and
          colored data wires carry values. When you build, every graph turns into readable Lua 5.4 inside a normal nanos world package.
        </p>
        <ol>
          <li>Create a project on the start screen, or open one with <Keys>Ctrl O</Keys>.</li>
          <li>Pick a graph in the tree on the left. New graphs, functions and classes are created with the plus buttons there.</li>
          <li>
            Press <Keys>Space</Keys> to add nodes, drag from a pin to connect it.
          </li>
          <li>
            Press <strong>Play</strong> or <Keys>Alt P</Keys> to build and start the local test server.
          </li>
        </ol>
      </>
    ),
  },
  {
    id: "editor",
    title: "Working in the graph editor",
    summary: "Adding and connecting nodes, the node list, comments and reroutes.",
    keywords: "editor node add connect wire pin palette list category comment reroute select",
    body: (
      <>
        <p>
          Press <Keys>Space</Keys> or right-click the canvas to open the node list. Type to search, for example <code>spawn character</code> or{" "}
          <code>print</code>. If you drag a wire into empty space, the list only shows nodes that fit that pin and connects the new node for you.
        </p>
        <p>
          The node list can be <strong>flat</strong>, sorted by relevance, or <strong>categorized</strong>, grouped into flow control, math, your project and
          the nanos world API by class. Switch it in <strong>Settings &gt; Editor</strong>.
        </p>
        <ul>
          <li>Nodes greyed out in the list run on the other side, for example a client only function in a server graph.</li>
          <li>
            Select nodes and press <Keys>C</Keys> to put a comment box around them.
          </li>
          <li>Double-click a wire to add a reroute point and tidy long wires.</li>
          <li>
            <Keys>Alt</Keys> + click a pin removes its wires.
          </li>
          <li>Changes save on their own. The Details panel on the right shows the selected node, its documentation and the Lua it produces.</li>
        </ul>
      </>
    ),
  },
  {
    id: "shortcuts",
    title: "Keyboard shortcuts",
    summary: "Every shortcut of the editor, the project and the app.",
    keywords: "keyboard shortcut hotkey keys keybinding",
    body: (
      <>
        {SHORTCUT_GROUPS.map((group) => (
          <section key={group.title} className={styles.shortcutGroup}>
            <h3>{group.title}</h3>
            <dl className={styles.shortcutList}>
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
      </>
    ),
  },
  {
    id: "scripting",
    title: "Events, functions and variables",
    summary: "How the building blocks of a package fit together.",
    keywords: "event function variable class struct enum macro interface dispatcher signature",
    body: (
      <>
        <ul>
          <li>
            <strong>Event graphs</strong> react to events of the game, like a player joining or a character dying.
          </li>
          <li>
            <strong>Functions</strong> have inputs and outputs you define. Mark a function as <em>pure</em> when it only computes a value, then it has no exec
            pins.
          </li>
          <li>
            <strong>Variables</strong> live in a graph. Drag them from the Variables panel onto the canvas to read or set them.
          </li>
          <li>
            <strong>Classes</strong> extend nanos world classes like Character or Prop with your own variables, methods and events.
          </li>
          <li>
            <strong>Structs</strong> and <strong>enums</strong> are your own data types, <strong>macros</strong> are reusable pieces of a graph and{" "}
            <strong>events</strong> send data between server and client.
          </li>
        </ul>
        <p>Right-click a function, variable or event and pick Find usages to see where it is used in the package.</p>
      </>
    ),
  },
  {
    id: "contexts",
    title: "Server, client and shared",
    summary: "Where your scripts run and how to talk between the sides.",
    keywords: "server client shared side authority network remote event context",
    body: (
      <>
        <p>
          <strong>Server</strong> graphs run on the server and decide what is true for everyone. <strong>Client</strong> graphs run on every player's game,
          for UI, input and effects. <strong>Shared</strong> graphs run on both.
        </p>
        <p>
          Some API functions only exist on one side. The node list marks them and the editor warns when one is used on the wrong side. To send data across,
          create an <strong>event</strong> with the direction server to client or client to server and use its Send node.
        </p>
      </>
    ),
  },
  {
    id: "custom-settings",
    title: "Configuration with custom settings",
    summary: "Make your game mode configurable: define settings, read them in graphs and set them per server.",
    keywords: "config configuration custom settings setting options config.toml custom_settings server parameter new game",
    body: (
      <>
        <p>
          nanos world game modes can declare <strong>custom settings</strong>, values a server owner sets without touching your scripts: a round time, the
          maximum number of props or whether PvP is on. NanosFlow writes them into the <code>[custom_settings]</code> section of your Package.toml.
        </p>
        <h3>1. Define the settings</h3>
        <p>
          Open <strong>Project settings</strong> (the gear in the title bar) and add settings to your game mode package. Every setting has a key like{" "}
          <code>max_props</code>, a label, a type and a default:
        </p>
        <ul>
          <li>
            <strong>Boolean</strong> is a checkbox, <strong>integer</strong> and <strong>floating</strong> are numbers, <strong>text</strong> is free text
          </li>
          <li>
            <strong>Select</strong> is a dropdown, its value is one of the options you list
          </li>
        </ul>
        <h3>2. Read them in a graph</h3>
        <p>
          Every setting gets its own node in the node list under <strong>Custom Settings</strong>, for example <em>Get Max Props</em>. It returns the value
          with the right type, and the default when nobody set one. Settings are read on the server, so use these nodes in server or shared graphs and
          send values to clients with an event when they need them.
        </p>
        <h3>3. Set them for a server</h3>
        <ul>
          <li>Players see them in the New Game screen of nanos world.</li>
          <li>
            Dedicated servers set them in <code>Config.toml</code> under <code>[custom_settings]</code>, for example <code>max_props = 500</code>.
          </li>
          <li>
            On the command line: <code>--custom_settings "max_props = 500, enable_pvp = false"</code>. This wins over everything else.
          </li>
          <li>
            For testing, set values in the <strong>Server</strong> panel under Settings. NanosFlow passes them to the test server on every start.
          </li>
        </ul>
      </>
    ),
  },
  {
    id: "testing",
    title: "Testing with the local server",
    summary: "Play, reload, the console and runtime errors on their nodes.",
    keywords: "test server play run reload console log runtime error steam install",
    body: (
      <>
        <p>
          The <strong>Server</strong> panel installs a nanos world server for you. After that, <strong>Play</strong> builds the project and starts it. While
          it runs, Play reloads only the changed packages, so you stay connected in game.
        </p>
        <ul>
          <li>Connect from nanos world with Direct Connect to the address shown in the panel.</li>
          <li>The console shows the server output and accepts commands.</li>
          <li>
            The <strong>Runtime</strong> panel follows the log. Lua errors and watched values appear on the nodes that caused them.
          </li>
        </ul>
      </>
    ),
  },
  {
    id: "debugging",
    title: "Breakpoints and the debugger",
    summary: "Pause the server on a node, look at values and step through.",
    keywords: "debug debugger breakpoint pause step watch port",
    body: (
      <>
        <p>
          Select a node and press <Keys>F9</Keys> to set a breakpoint. When the server reaches it, it pauses and the debug bar shows the values of the node.
          Continue or step into, over and out of functions from there.
        </p>
        <p>
          The game talks to NanosFlow over a local port, 47615 by default. If another program already uses it, pick another port in{" "}
          <strong>Settings &gt; Debugging</strong> and build again, because the port is compiled into the debug build.
        </p>
      </>
    ),
  },
  {
    id: "sessions",
    title: "Sessions: working together",
    summary: "Host a project, let friends edit with you and play on your test server.",
    keywords: "session host join relay together pair multiplayer friends code sync",
    body: (
      <>
        <p>
          A session shares your project with guests through a relay. Guests see your graphs live, edit with you and their cursors show up in the editor.
          When you press Play, guests can join your test server with one click.
        </p>
        <ol>
          <li>Enter the relay address in the session panel. You can host your own relay with the Docker image from the README.</li>
          <li>Host the session and send the code to your friends.</li>
          <li>Admit them when they ask to join.</li>
        </ol>
      </>
    ),
  },
  {
    id: "agents",
    title: "AI agents",
    summary: "Let Claude Code, Cursor, Codex and others build graphs with you.",
    keywords: "ai agent mcp claude cursor codex copilot vs code gemini windsurf assistant",
    body: (
      <>
        <p>
          NanosFlow can start a local MCP server. AI agents then read your graphs, add and connect nodes, build the project and use the test server. Every
          change shows up live, and you can undo each step of an agent with <Keys>Ctrl Z</Keys>.
        </p>
        <p>
          Set up a client in <strong>Settings &gt; AI agents</strong> with one click, or copy the command. There you can also switch off single abilities,
          for example editing graphs or controlling the test server.
        </p>
      </>
    ),
  },
  {
    id: "version-control",
    title: "Version control",
    summary: "Why projects work well with Git and how merges are handled.",
    keywords: "git version control merge conflict commit diff",
    body: (
      <>
        <p>
          Every node is stored in its own small file, so two people rarely change the same file. If Git still reports a conflict, NanosFlow shows the graph
          with both versions and lets you pick per node.
        </p>
        <p>
          The <code>.nanosflow</code> folder holds settings of your computer, like the test server path. It is ignored by Git.
        </p>
      </>
    ),
  },
];

export function findTopic(id: string): HelpTopic {
  return HELP_TOPICS.find((topic) => topic.id === id) ?? HELP_TOPICS[0];
}
