import type { ReactNode } from 'react';
import en from './locales/en.json';
import fr from './locales/fr.json';
import es from './locales/es.json';
import de from './locales/de.json';

export const locales = ['en', 'fr', 'es', 'de'] as const;
export type Locale = typeof locales[number];
export type LanguagePreference = 'system' | Locale;
type Entry = string | Record<string, string>;
type Catalog = Record<string, Entry>;
export const catalogs: Record<Locale, Catalog> = { en, fr, es, de };
const tags = { en: 'en-GB', fr: 'fr-FR', es: 'es-ES', de: 'de-DE' };
export function resolveLocale(preference: LanguagePreference): Locale {
  if (preference !== 'system') return preference;
  const languages = typeof navigator === 'undefined' ? [] : [...(navigator.languages ?? []), navigator.language];
  for (const language of languages) {
    const primary = language?.split('-')[0].toLowerCase();
    if (locales.includes(primary as Locale)) return primary as Locale;
  }
  return 'en';
}
export function readPreference(): LanguagePreference {
  try {
    const saved = localStorage.getItem('sprawl.language');
    if (locales.includes(saved as Locale)) return saved as Locale;
  } catch {}
  return 'system';
}
export let activeLocale = resolveLocale(readPreference());
export let intlLocale = tags[activeLocale];
export function setPreference(preference: LanguagePreference) {
  activeLocale = resolveLocale(preference);
  intlLocale = tags[activeLocale];
  if (typeof document !== 'undefined' && document.documentElement) document.documentElement.lang = activeLocale;
  try { localStorage.setItem('sprawl.language', preference); } catch {}
}
function lookup(key: string, count: unknown): string {
  const entry = catalogs[activeLocale][key] ?? catalogs.en[key];
  if (!entry) return key;
  return typeof entry === 'string' ? entry : entry[new Intl.PluralRules(activeLocale).select(typeof count === 'number' ? count : 0)] ?? entry.other;
}
export function t(key: string, params: Record<string, string | number> = {}): string {
  return lookup(key, params.count).replace(/\{(\w+)\}/g, (match, name: string) => {
    const value = params[name];
    return value === undefined ? match : typeof value === 'number' ? value.toLocaleString(intlLocale) : value;
  });
}
export function rich(key: string, params: Record<string, ReactNode> = {}): ReactNode[] {
  return lookup(key, params.count).split(/(\{\w+\})/g).map(part => {
    const name = /^\{(\w+)\}$/.exec(part)?.[1];
    const value = name === undefined ? undefined : params[name];
    return value === undefined ? part : typeof value === 'number' ? value.toLocaleString(intlLocale) : value;
  });
}
