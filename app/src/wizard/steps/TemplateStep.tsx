import { File, FileCode2, Gamepad2, Map } from "lucide-react";
import { ChoiceCard, ChoiceGrid } from "../../components/Choice";
import type { ProjectTemplate } from "../../types";
import styles from "../Wizard.module.css";
import type { StepProps } from "./StepProps";

const TEMPLATES: { id: ProjectTemplate; title: string; description: string; icon: typeof File; tint: string; badge?: string }[] = [
  {
    id: "game-mode",
    title: "Game Mode",
    description: "A complete game. Starts with server and client graphs wired to Package Load, Player Ready and Unload.",
    icon: Gamepad2,
    tint: "var(--violet)",
    badge: "Popular",
  },
  {
    id: "script",
    title: "Script",
    description: "An add-on that runs next to any game mode, like an admin menu, HUD or a shared library.",
    icon: FileCode2,
    tint: "var(--blue)",
  },
  {
    id: "map",
    title: "Map",
    description: "Loads a map asset and can script what lives on it, such as spawn points, props and vehicles.",
    icon: Map,
    tint: "var(--cyan)",
  },
  {
    id: "blank",
    title: "Blank",
    description: "One empty server graph. For when you already know exactly what you want.",
    icon: File,
    tint: "var(--text-muted)",
  },
];

export function TemplateStep({ draft }: StepProps) {
  return (
    <div className={styles.stepBody}>
      <ChoiceGrid columns={2}>
        {TEMPLATES.map((template) => (
          <ChoiceCard
            key={template.id}
            title={template.title}
            description={template.description}
            icon={<template.icon size={18} />}
            tint={template.tint}
            badge={template.badge}
            selected={draft.options.template === template.id}
            onSelect={() => draft.setTemplate(template.id)}
          />
        ))}
      </ChoiceGrid>
    </div>
  );
}
