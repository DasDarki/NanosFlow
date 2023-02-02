import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './App.vue';

import '@/styles/index.scss';
import ConfigManager from "@/code/ConfigManager";
import {fs, path} from "@tauri-apps/api";
import {setupI18n} from "@/i18n";

const app = createApp(App);

app.use(createPinia());

function initializeDocumentEvents() {
  if (import.meta.env.MODE !== 'development') {
    document.onkeydown = function (e) {
      return (e.which || e.keyCode) != 116;
    };
  }

  document.addEventListener('contextmenu', e => {
    if (!ConfigManager.config.isDevMode) {
      e.preventDefault();
      return false;
    }
  }, {capture: true})

  document.addEventListener('selectstart', e => {
    if (!ConfigManager.config.isDevMode) {
      e.preventDefault();
      return false;
    }
  }, {capture: true})
}

async function initializeDataDir() {
  const dataDir = await path.appDataDir();
  if (!await fs.exists(dataDir)) {
    await fs.createDir(dataDir);
  }
}

async function initializeApp() {
  await initializeDataDir();
  await ConfigManager.initialize();
  app.use(await setupI18n(ConfigManager.config.locale));

  initializeDocumentEvents();

  app.mount('#app');
}

(async () => await initializeApp())();
