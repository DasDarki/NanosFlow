export interface Project {
  name: string;
  author: string;
  version: string;
  packageType: PackageType;
  packageSettings: PackageSettings;
  scripts: {
    shared: ScriptElement[];
    client: ScriptElement[];
    server: ScriptElement[];
  };
}

export enum PackageType {
  Script = 'script',
  GameMode = 'game-mode'
}

export interface PackageSettings {
  forceNoMapPackage: boolean;
  autoCleanup: boolean;
  loadLevelEntities: boolean;
  compatibilityVersion: string;
  packageRequirements: string[];
  assetsRequirements: string[];
  compatibleGameModes: string[];
  compatibleMaps: string[];
  custom: {[key: string]: CustomPackageSettingType};
}

export interface Script {
  name: string;

}

export interface ScriptFolder {
  name: string;
  elements: ScriptElement[];
}

export type CustomPackageSettingType = boolean | number | string;
export type ScriptElement = ScriptFolder | Script;
