import type { ScriptContext } from "../types";
import { expandDefinition } from "./dynamic";
import type { CatalogPayload, ClassInfo, EnumInfo, NodeDefinition, PinDefinition, PinType, ProjectClass, ProjectEnum, ProjectStruct, Side, TypeExpr } from "./model";

export type TypeCategory =
  | "boolean"
  | "number"
  | "integer"
  | "string"
  | "asset-path"
  | "table"
  | "function"
  | "any"
  | "enum"
  | "struct"
  | "object";

export const STRUCT_FIELDS: Record<string, string[]> = {
  Vector: ["X", "Y", "Z"],
  Vector2D: ["X", "Y"],
  Rotator: ["Pitch", "Yaw", "Roll"],
  Color: ["R", "G", "B", "A"],
  Quat: ["X", "Y", "Z", "W"],
};

export function sideRunsIn(side: Side, context: ScriptContext): boolean {
  return side === "both" || side === context;
}

export function baseType(type: TypeExpr): TypeExpr {
  return type.kind === "optional" || type.kind === "variadic" ? baseType(type.of) : type;
}

export function formatType(type: TypeExpr): string {
  switch (type.kind) {
    case "named":
      return type.of === "$T" ? "Wildcard" : type.of;
    case "array":
      return `${formatType(type.of)}[]`;
    case "optional":
      return `${formatType(type.of)}?`;
    case "variadic":
      return `${formatType(type.of)}...`;
    case "union":
      return type.of.map(formatType).join("|");
  }
}

interface SearchEntry {
  definition: NodeDefinition;
  title: string;
  haystack: string;
}

export interface PinReference {
  definition: PinDefinition;
  direction: "input" | "output";
}

export class NodeCatalog {
  readonly revision: string;
  readonly size: number;
  private readonly definitions = new Map<string, NodeDefinition>();
  private readonly classes = new Map<string, ClassInfo>();
  private readonly enums = new Map<string, EnumInfo>();
  private readonly searchEntries: SearchEntry[];

  private readonly payload: CatalogPayload;

  private readonly projectClasses: ProjectClass[];
  private readonly projectStructs: Set<string>;
  private readonly projectEnums: Set<string>;

  constructor(
    payload: CatalogPayload,
    extras: NodeDefinition[] = [],
    projectClasses: ProjectClass[] = [],
    projectStructs: ProjectStruct[] = [],
    projectEnums: ProjectEnum[] = [],
  ) {
    this.payload = payload;
    this.projectClasses = projectClasses;
    this.projectStructs = new Set(projectStructs.map((structure) => structure.name));
    this.projectEnums = new Set(projectEnums.map((enumeration) => enumeration.name));
    this.revision = payload.revision;
    const nodes = [...extras, ...payload.nodes];
    this.size = nodes.length;
    for (const node of nodes) {
      if (!this.definitions.has(node.id)) {
        this.definitions.set(node.id, node);
      }
    }
    for (const info of payload.classes) {
      this.classes.set(info.name, info);
    }
    for (const project of projectClasses) {
      this.classes.set(project.name, { name: project.name, kind: "class", parents: project.parents, isBase: false, inheritable: false });
    }
    for (const info of payload.enums) {
      this.enums.set(info.name, info);
    }
    for (const enumeration of projectEnums) {
      this.enums.set(enumeration.name, {
        name: enumeration.name,
        values: enumeration.values.map((value, index) => ({ key: value.id, value: String(index + 1), label: value.name })),
      });
    }
    this.searchEntries = [...this.definitions.values()].map((definition) => ({
      definition,
      title: definition.title.toLowerCase(),
      haystack: [definition.title, definition.subtitle ?? "", definition.category, ...(definition.keywords ?? [])]
        .join(" ")
        .toLowerCase(),
    }));
  }

  extend(
    extras: NodeDefinition[],
    projectClasses: ProjectClass[] = [],
    projectStructs: ProjectStruct[] = [],
    projectEnums: ProjectEnum[] = [],
  ): NodeCatalog {
    return new NodeCatalog(this.payload, extras, projectClasses, projectStructs, projectEnums);
  }

  projectEnumNames(): string[] {
    return [...this.projectEnums];
  }

  projectStructNames(): string[] {
    return [...this.projectStructs];
  }

  projectClassNames(): string[] {
    return this.projectClasses.map((project) => project.name);
  }

  apiClassNames(): string[] {
    return this.payload.classes.filter((info) => info.inheritable).map((info) => info.name);
  }

  classNames(kind: ClassInfo["kind"]): string[] {
    return [...this.classes.values()].filter((info) => info.kind === kind).map((info) => info.name);
  }

  enumNames(): string[] {
    return [...this.enums.keys()];
  }

  get(id: string): NodeDefinition | undefined {
    return this.definitions.get(id);
  }

  definitionFor(nodeType: string, properties: Record<string, unknown> | undefined): NodeDefinition | undefined {
    const definition = this.definitions.get(nodeType);
    return definition && expandDefinition(definition, properties ?? {});
  }

  enumInfo(name: string): EnumInfo | undefined {
    return this.enums.get(name);
  }

  category(name: string): TypeCategory {
    switch (name) {
      case "boolean":
        return "boolean";
      case "number":
      case "float":
        return "number";
      case "integer":
        return "integer";
      case "string":
        return "string";
      case "table":
        return "table";
      case "function":
        return "function";
    }
    if (this.projectStructs.has(name)) {
      return "table";
    }
    if (name.endsWith("Path") && !this.classes.has(name)) {
      return "asset-path";
    }
    if (this.enums.has(name)) {
      return "enum";
    }
    const info = this.classes.get(name);
    if (info?.kind === "struct") {
      return "struct";
    }
    if (info?.kind === "class") {
      return "object";
    }
    return "any";
  }

  typeCategory(type: TypeExpr): TypeCategory {
    const base = baseType(type);
    if (base.kind === "named") {
      return this.category(base.of);
    }
    if (base.kind === "array") {
      return "table";
    }
    if (base.kind === "union") {
      const categories = new Set(base.of.map((member) => this.typeCategory(member)));
      return categories.size === 1 ? [...categories][0] : "any";
    }
    return "any";
  }

  isSubclassOf(name: string, ancestor: string): boolean {
    return name === ancestor || (this.classes.get(name)?.parents.includes(ancestor) ?? false);
  }

  isAssignable(from: TypeExpr, to: TypeExpr): boolean {
    if (to.kind === "variadic" || to.kind === "optional") {
      return this.isAssignable(from, to.of);
    }
    if (from.kind === "variadic" || from.kind === "optional") {
      return this.isAssignable(from.of, to);
    }
    if (from.kind === "union") {
      return from.of.every((member) => this.isAssignable(member, to));
    }
    if (to.kind === "union") {
      return to.of.some((member) => this.isAssignable(from, member));
    }
    if (from.kind === "array" && to.kind === "array") {
      return this.isAssignable(from.of, to.of);
    }
    if (from.kind === "array" && to.kind === "named") {
      const category = this.category(to.of);
      return category === "table" || category === "any";
    }
    if (from.kind === "named" && to.kind === "array") {
      const category = this.category(from.of);
      return category === "table" || category === "any";
    }
    if (from.kind === "named" && to.kind === "named") {
      return this.isNameAssignable(from.of, to.of);
    }
    return false;
  }

  private isNameAssignable(from: string, to: string): boolean {
    if (from === to) {
      return true;
    }
    const fromCategory = this.category(from);
    const toCategory = this.category(to);
    if (fromCategory === "any" || toCategory === "any") {
      return true;
    }
    const numeric = (category: TypeCategory) => category === "number" || category === "integer";
    if (numeric(fromCategory) && numeric(toCategory)) {
      return true;
    }
    if (fromCategory === "enum" && numeric(toCategory)) {
      return true;
    }
    if (
      (fromCategory === "string" && toCategory === "asset-path") ||
      (fromCategory === "asset-path" && toCategory === "string")
    ) {
      return true;
    }
    if (fromCategory === "object" && toCategory === "object") {
      return this.isSubclassOf(from, to);
    }
    if (fromCategory === "table" && toCategory === "table") {
      return from === "table" || to === "table";
    }
    return false;
  }

  canConnect(output: PinType, input: PinType): boolean {
    if (output.kind === "exec" || input.kind === "exec") {
      return output.kind === input.kind;
    }
    return this.isAssignable(output.type, input.type);
  }

  firstCompatiblePin(definition: NodeDefinition, pinType: PinType, direction: "input" | "output"): PinDefinition | undefined {
    if (direction === "input") {
      return definition.inputs.find((pin) => this.canConnect(pinType, pin.type));
    }
    return definition.outputs.find((pin) => this.canConnect(pin.type, pinType));
  }

  private filterScore(definition: NodeDefinition, filter: { pinType: PinType; direction: "input" | "output" }): number | null {
    const pin = this.firstCompatiblePin(definition, filter.pinType, filter.direction);
    if (!pin) {
      return null;
    }
    if (pin.type.kind === "exec" || filter.pinType.kind === "exec") {
      return 0;
    }
    if (formatType(baseType(pin.type.type)) === formatType(baseType(filter.pinType.type))) {
      return 3;
    }
    return this.typeCategory(pin.type.type) === "any" ? -1 : 1;
  }

  search(query: string, filter?: { pinType: PinType; direction: "input" | "output" }): NodeDefinition[] {
    const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
    const scored: { definition: NodeDefinition; score: number }[] = [];
    for (const entry of this.searchEntries) {
      const bonus = filter ? this.filterScore(entry.definition, filter) : 0;
      if (bonus === null) {
        continue;
      }
      let score = bonus;
      let matchesAll = true;
      for (const term of terms) {
        if (entry.title.startsWith(term)) {
          score += 6;
        } else if (entry.title.includes(` ${term}`)) {
          score += 4;
        } else if (entry.title.includes(term)) {
          score += 2;
        } else if (entry.haystack.includes(term)) {
          score += 1;
        } else {
          matchesAll = false;
          break;
        }
      }
      if (matchesAll) {
        scored.push({ definition: entry.definition, score: score - entry.title.length / 200 });
      }
    }
    scored.sort((left, right) => right.score - left.score || left.definition.title.localeCompare(right.definition.title));
    return scored.map((entry) => entry.definition);
  }
}
