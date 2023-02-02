import {createI18n} from "vue-i18n";
import {nextTick} from "vue";
import type {I18n} from "vue-i18n";

let i18n: I18n;

const SUPPORTED_LOCALES = ['en'];
export const LOCALE_NAMES = {
  en: 'English'
};

export async function setupI18n(locale: string): Promise<I18n> {
  i18n = createI18n({locale, fallbackLocale: locale});

  await loadLocaleMessages(i18n, locale);
  setI18nLanguage(i18n, locale);

  return i18n;
}

export async function selectLocale(locale: string) {
  if (!SUPPORTED_LOCALES.includes(locale)) {
    return;
  }

  if (!i18n.global.availableLocales.includes(locale)) {
    await loadLocaleMessages(i18n, locale);
  }

  setI18nLanguage(i18n, locale);
}

function setI18nLanguage(i18n: any, locale: string) {
  if (i18n.mode === 'legacy') {
    i18n.global.locale = locale
  } else {
    i18n.global.locale.value = locale
  }

  document.querySelector('html')?.setAttribute('lang', locale)
}

async function loadLocaleMessages(i18n: any, locale: string) {
  const messages = await import(/* webpackChunkName: "locale-[request]" */ `./locales/${locale}.json`);
  i18n.global.setLocaleMessage(locale, messages.default);
  return nextTick();
}
