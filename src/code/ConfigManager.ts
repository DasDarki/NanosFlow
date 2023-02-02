import type {Config} from "@/@types/internal";
import {fs, path} from "@tauri-apps/api";

export default class ConfigManager {

  private static _config: Config;

  public static get config(): Config {
    return this._config;
  }

  public static async initialize(): Promise<void> {
    const configPath = await path.join(await path.appDataDir(), 'config.json');
    if (await fs.exists(configPath)) {
      this._config = JSON.parse(await fs.readTextFile(configPath));
    } else {
      this._config = await this.generateDefaultConfig(configPath);
    }
  }

  private static async generateDefaultConfig(path: string): Promise<Config> {
    const config: Config = {
      isDevMode: import.meta.env.MODE === 'development',
      recentProjects: [],
      locale: "en"
    };

    await fs.writeTextFile(path, JSON.stringify(config, null, 2));

    return config;
  }
}
