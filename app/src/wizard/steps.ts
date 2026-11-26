import type { OptionField } from "../types";

export type StepId = "template" | "project" | "package" | "scripts" | "review";

export interface StepDefinition {
  id: StepId;
  title: string;
  subtitle: string;
  heading: string;
  description: string;
  fields: OptionField[];
}

export const STEPS: StepDefinition[] = [
  {
    id: "template",
    title: "Template",
    subtitle: "What are you building?",
    heading: "Pick a starting point",
    description: "Templates only decide what gets set up first. You can add packages, graphs and scripts later.",
    fields: ["template"],
  },
  {
    id: "project",
    title: "Project",
    subtitle: "Name and location",
    heading: "Name your project",
    description: "The project folder holds your graphs and settings. It is safe to put under version control.",
    fields: ["name", "location", "folderName"],
  },
  {
    id: "package",
    title: "Package",
    subtitle: "nanos world settings",
    heading: "Configure the package",
    description: "These values end up in the generated Package.toml that nanos world reads.",
    fields: ["packageId", "packageType", "author", "version", "compatibilityVersion", "mapAsset"],
  },
  {
    id: "scripts",
    title: "Scripts & Sharing",
    subtitle: "Contexts, API and Git",
    heading: "Where does your code run?",
    description: "Each context gets its own graph. NanosFlow checks every node against the side it runs on.",
    fields: ["contexts", "apiChannel", "initializeGit"],
  },
  {
    id: "review",
    title: "Review",
    subtitle: "Check and create",
    heading: "Ready to create",
    description: "This is everything NanosFlow is about to write to disk. Nothing outside the new folder is touched.",
    fields: [],
  },
];
