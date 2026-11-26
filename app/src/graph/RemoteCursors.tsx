import { useViewport, ViewportPortal } from "@xyflow/react";
import type { CSSProperties } from "react";
import styles from "./RemoteCursors.module.css";

export interface RemotePeer {
  peer: number;
  name: string;
  color: string;
  x: number;
  y: number;
  selection: string[];
}

const PEER_COLORS = ["#f472b6", "#38bdf8", "#facc15", "#a78bfa", "#fb923c", "#4ade80", "#f87171", "#2dd4bf"];

export function peerColor(peer: number): string {
  return PEER_COLORS[peer % PEER_COLORS.length];
}

export function RemoteCursors({ peers }: { peers: RemotePeer[] }) {
  const { zoom } = useViewport();
  return (
    <ViewportPortal>
      {peers.map((peer) => (
        <div
          key={peer.peer}
          className={styles.cursor}
          style={{ transform: `translate(${peer.x}px, ${peer.y}px) scale(${1 / zoom})`, "--peer": peer.color } as CSSProperties}
          aria-label={`${peer.name}'s cursor`}
        >
          <svg width="16" height="18" viewBox="0 0 16 18" aria-hidden>
            <path d="M1 1 L1 15 L5 11 L8 17 L10.5 16 L7.5 10 L13 10 Z" />
          </svg>
          <span className={styles.name}>{peer.name}</span>
        </div>
      ))}
    </ViewportPortal>
  );
}
