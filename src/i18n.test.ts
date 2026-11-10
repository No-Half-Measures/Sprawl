import { afterEach, describe, expect, it, vi } from 'vitest';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { activeLocale, catalogs, intlLocale, locales, readPreference, resolveLocale, rich, setPreference, t } from './i18n';
import { ageBuckets, categories, count, formatAge, formatSize, percent } from './model';

const placeholders = (text: string) => [...new Set([...text.matchAll(/\{(\w+)\}/g)].map(m => m[1]))].sort();

afterEach(() => { vi.unstubAllGlobals(); setPreference('system'); });

describe('shared catalogs', () => {
  it('has matching keys, plural forms and placeholders in every language', () => {
    const samples = new Set([0, 1, 2, 999999, 1000000]);
    for (let n = 0; n <= 2000; n++) samples.add(n);
    for (let n = 10000; n <= 1000000; n += 10000) samples.add(n);
    for (const locale of locales) {
      expect(Object.keys(catalogs[locale]).sort()).toEqual(Object.keys(catalogs.en).sort());
      const rules = new Intl.PluralRules(locale);
      const required = new Set(['other', ...[...samples].map(n => rules.select(n))]);
      for (const [key, english] of Object.entries(catalogs.en)) {
        const entry = catalogs[locale][key];
        expect(typeof entry, `${locale}: ${key}`).toBe(typeof english);
        const englishForms = typeof english === 'string' ? [english] : Object.values(english);
        const expected = placeholders(englishForms[0]);
        for (const form of englishForms) expect(placeholders(form), key).toEqual(expected);
        if (typeof entry !== 'string') {
          for (const category of required) expect(entry, `${locale}: ${key}`).toHaveProperty(category);
        }
        for (const text of typeof entry === 'string' ? [entry] : Object.values(entry)) {
          expect(text.trim(), `${locale}: ${key}`).not.toBe('');
          expect(placeholders(text), `${locale}: ${key}`).toEqual(expected);
        }
      }
    }
  });

  it('covers literal source keys and dynamic categories, age buckets, sorts and counts', () => {
    const frontend = import.meta.glob('./**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    const backend = import.meta.glob('../src-tauri/src/**/*.rs', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    const keys = new Set<string>();
    for (const [file, source] of Object.entries({ ...frontend, ...backend })) {
      if (/\.test\.tsx?$/.test(file)) continue;
      for (const match of source.matchAll(/\b(?:t|rich|with_args)\(\s*['"]([^'"]+)['"]/g)) keys.add(match[1]);
      // Also cover conditional key arguments.
      for (const match of source.matchAll(/['"]((?:errors|results)\.[\w.]+)['"]/g)) keys.add(match[1]);
    }
    for (const category of categories) keys.add(`category.${category.id}`);
    for (const bucket of ageBuckets) keys.add(`ageBucket.${bucket.id}`);
    for (const sort of ['size', 'name', 'modified']) keys.add(`sort.${sort}`);
    for (const unit of ['B', 'KB', 'MB', 'GB', 'TB', 'PB']) keys.add(`units.${unit}`);
    for (const [file, source] of Object.entries(frontend)) {
      if (/\.test\.tsx?$/.test(file)) continue;
      for (const match of source.matchAll(/\bcount\([^(),\n]+,\s*'([^']+)'\)/g)) keys.add(`counts.${match[1].replace(/ /g, '_')}`);
    }
    for (const key of keys) expect(catalogs.en, key).toHaveProperty(key);
  });
});

describe('locale lookup and formatting', () => {
  it('reorders rich placeholders while preserving nodes and English emphasis', () => {
    const used = createElement('strong', { key: 'used' }, '1 GB');
    setPreference('en');
    expect(renderToStaticMarkup(rich('home.used', { used, total: '2 GB' }))).toBe('<strong>1 GB</strong> used of 2 GB');
    setPreference('de');
    expect(renderToStaticMarkup(rich('home.used', { used, total: '2 GB' }))).toBe('<strong>1 GB</strong> von 2 GB belegt');
    const size = createElement('strong', { key: 'size' }, '1 Go');
    setPreference('fr');
    expect(renderToStaticMarkup(rich('results.used', { size }))).toBe('<strong>1 Go</strong> utilisés');
    expect(rich('delete.question', { name: used })[1]).toBe(used);
  });

  it('shares fallback, plural selection and number formatting with text templates', () => {
    setPreference('fr');
    expect(rich('delete.question', { name: '{size}' }).join('')).toBe(t('delete.question', { name: '{size}' }));
    expect(rich('delete.question').join('')).toBe(t('delete.question'));
    expect(rich('missing.key')).toEqual(['missing.key']);
    const entry = catalogs.fr['home.used'];
    delete catalogs.fr['home.used'];
    try { expect(rich('home.used', { used: '1 GB', total: '2 GB' }).join('')).toBe('1 GB used of 2 GB'); }
    finally { catalogs.fr['home.used'] = entry; }
    for (const locale of locales) {
      setPreference(locale);
      for (const count of [0, 1, 2, 1000000]) {
        const params = { count, name: 'scan.csv' };
        expect(rich('notice.exported', params).join('')).toBe(t('notice.exported', params));
        expect(t('notice.exported', params)).not.toContain('{');
      }
    }
    setPreference('en');
    expect(t('notice.exported', { count: 1, name: 'scan.csv' })).toBe('Exported 1 item to scan.csv.');
    expect(t('notice.exported', { count: 1234, name: 'scan.csv' })).toBe('Exported 1,234 items to scan.csv.');
  });

  it('substitutes once, preserves unknown placeholders and falls back to English then the key', () => {
    setPreference('fr');
    expect(t('delete.question', { name: '{size}' })).toBe('Supprimer {size} ?');
    expect(t('delete.question')).toBe('Supprimer {name} ?');
    const entry = catalogs.fr['home.scanDrive'];
    delete catalogs.fr['home.scanDrive'];
    try { expect(t('home.scanDrive')).toBe('Scan drive'); }
    finally { catalogs.fr['home.scanDrive'] = entry; }
    expect(t('missing.key')).toBe('missing.key');
  });

  it('selects locale plural forms and formats numbers, including French zero and many', () => {
    setPreference('fr');
    expect(count(0, 'file')).toBe('0 fichier');
    expect(count(1, 'file')).toBe('1 fichier');
    expect(count(2, 'file')).toBe('2 fichiers');
    expect(count(1000000, 'file')).toBe('1\u202f000\u202f000 fichiers');
    setPreference('en');
    expect(count(0, 'file')).toBe('0 files');
    expect(count(12345, 'file')).toBe('12,345 files');
  });

  it('formats sizes, ages and percentages in French', () => {
    setPreference('fr');
    expect(formatSize(0)).toBe('0 o');
    expect(formatSize(1536)).toBe('1,5 Ko');
    expect(formatSize(1024 ** 3)).toBe('1 Go');
    const now = Date.UTC(2026, 0, 1);
    expect(formatAge(now, now)).toBe('à l’instant');
    expect(formatAge(now - 60000, now)).toBe('il y a 1 minute');
    expect(formatAge(now - 120000, now)).toBe('il y a 2 minutes');
    expect(formatAge(now - 86400000, now)).toBe('hier');
    expect(formatAge(now - 60 * 86400000, now)).toBe('il y a 2 mois');
    expect(percent(1, 4)).toBe('25\u00a0%');
  });

  it('resolves supported primary subtags in preference order and handles blocked storage', () => {
    vi.stubGlobal('navigator', { languages: ['it-IT', 'de-AT', 'fr-FR'], language: 'es-ES' });
    expect(resolveLocale('system')).toBe('de');
    expect(resolveLocale('fr')).toBe('fr');
    vi.stubGlobal('navigator', { languages: ['ja'], language: 'it' });
    expect(resolveLocale('system')).toBe('en');
    vi.stubGlobal('navigator', { language: 'es-MX' });
    expect(resolveLocale('system')).toBe('es');
    vi.stubGlobal('localStorage', { getItem: () => { throw Error('blocked'); }, setItem: () => { throw Error('blocked'); } });
    expect(readPreference()).toBe('system');
    expect(() => setPreference('de')).not.toThrow();
    expect(activeLocale).toBe('de');
    expect(intlLocale).toBe('de-DE');
  });
});
