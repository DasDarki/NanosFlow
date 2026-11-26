import type { CSSProperties } from "react";
import type { ScriptContext } from "../types";
import { formatType, sideRunsIn, type NodeCatalog } from "./catalog";
import type { NodeDefinition, PinDefinition } from "./model";
import { definitionColor, KIND_LABELS, pinColor } from "./pinStyle";
import styles from "./NodeDocs.module.css";

function PinList({ title, pins, catalog }: { title: string; pins: PinDefinition[]; catalog: NodeCatalog }) {
  const dataPins = pins.filter((pin) => pin.type.kind === "data");
  if (dataPins.length === 0) {
    return null;
  }
  return (
    <div className={styles.section}>
      <div className={styles.sectionTitle}>{title}</div>
      {dataPins.map((pin) => (
        <div key={pin.name} className={styles.pin}>
          <div className={styles.pinHead}>
            <span className={styles.dot} style={{ "--pin": pinColor(catalog, pin.type) } as CSSProperties} />
            <span className={styles.pinName}>{pin.label || pin.name}</span>
            <code className={styles.pinType}>{pin.type.kind === "data" ? formatType(pin.type.type) : "exec"}</code>
            {pin.optional && pin.default !== undefined && <span className={styles.optional}>= {pin.default || '""'}</span>}
          </div>
          {pin.description && <p className={styles.pinDescription}>{pin.description}</p>}
        </div>
      ))}
    </div>
  );
}

export function NodeDocs({ definition, catalog, context }: { definition: NodeDefinition; catalog: NodeCatalog; context: ScriptContext }) {
  const available = sideRunsIn(definition.side, context);
  return (
    <div className={styles.docs}>
      <div className={styles.kind} style={{ "--kind": definitionColor(definition) } as CSSProperties}>
        {definition.id.startsWith("macro/") ? "Macro" : KIND_LABELS[definition.kind]}
        <span>{definition.category}</span>
      </div>
      <h3 className={styles.title}>{definition.title}</h3>
      {definition.subtitle && <div className={styles.subtitle}>{definition.subtitle}</div>}
      <div className={styles.side} data-available={available}>
        {definition.side === "both"
          ? "Runs on server and client"
          : `${definition.side === "server" ? "Server" : "Client"} only${available ? "" : `, not available in ${context} graphs`}`}
      </div>
      {definition.description && <p className={styles.description}>{definition.description}</p>}
      <PinList title="Inputs" pins={definition.inputs} catalog={catalog} />
      <PinList title="Outputs" pins={definition.outputs} catalog={catalog} />
      <code className={styles.id}>{definition.id}</code>
    </div>
  );
}
