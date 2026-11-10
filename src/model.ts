import { t, intlLocale } from './i18n';
import categories from './categories.json';
export { categories };
export interface TreeNode { id: number | null; name: string; kind: 'dir' | 'file' | 'other'; size: number; allocated: number; modified?: number; files: number; folders: number; category?: string; children?: TreeNode[] }
export interface LargeFile { id: number; name: string; relativePath: string; size: number; allocated: number; modified?: number; category: string }
export interface GrownFile { id: number; name: string; relativePath: string; size: number; allocated: number; modified?: number; previousSize: number | null }
export interface Changes { since: number; previousSize: number | null; children: { id: number; previousSize: number | null }[]; removed: { items: number; bytes: number }; grown: GrownFile[] }
export interface SearchMatch { id: number; name: string; relativePath: string; kind: 'dir' | 'file'; size: number; allocated: number; modified?: number; category?: string }
export interface SearchResults { total: number; results: SearchMatch[] }
export interface Breakdown { category: string; bytes: number; files: number }
export interface Drive { mount: string; label: string; total: number; free: number }
export interface AdminStatus { supported: boolean; elevated: boolean; alwaysElevate: boolean; declined: boolean; contextMenuEnabled: boolean }
export interface SavedScan { version: number; path: string; scannedAt: number; size: number; files: number; folders: number; recycled?: number; deleted?: number; errors: number; driveFree: number | null; driveTotal: number | null }
export interface Progress { files: number; folders: number; bytes: number; errors: number; currentPath: string }
export const emptyProgress: Progress = { files: 0, folders: 0, bytes: 0, errors: 0, currentPath: '' };
export function count(n: number, noun: string): string { return t(`counts.${noun.replace(/ /g, '_')}`, { count: n }); }
export function refreshTrails(trail: TreeNode[], history: TreeNode[][], ids: number[], nodes: (TreeNode | null)[]): { trail: TreeNode[]; history: TreeNode[][] } {
  const refreshed = new Map(ids.map((id, i) => [id, nodes[i]]));
  const attached = (node: TreeNode) => node.id !== null && refreshed.get(node.id) != null;
  const replace = (node: TreeNode) => refreshed.get(node.id!)!;
  const detached = trail.findIndex(node => !attached(node));
  return {
    trail: trail.slice(0, detached < 0 ? trail.length : detached).map(replace),
    history: history.filter(path => path.every(attached)).map(path => path.map(replace)),
  };
}
export function isDriveRoot(path: string, mount: string): boolean {
  return path.replace(/[\\/]+$/, '').toLowerCase() === mount.replace(/[\\/]+$/, '').toLowerCase();
}
export function formatAge(ms: number, now = Date.now()): string {
  const minutes = Math.floor(Math.max(0, now - ms) / 60000);
  if (minutes < 1) return t('age.now');
  if (minutes < 60) return t('age.minute', { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t('age.hour', { count: hours });
  const days = Math.floor(hours / 24);
  if (days === 1) return t('age.yesterday');
  if (days < 7) return t('age.day', { count: days });
  if (days < 30) return t('age.week', { count: Math.floor(days / 7) });
  if (days < 365) return t('age.month', { count: Math.floor(days / 30) });
  return t('age.year', { count: Math.floor(days / 365) });
}
export function formatScanDate(ms: number): string {
  const date = new Date(ms);
  const day = date.toLocaleDateString(intlLocale, { day: 'numeric', month: 'short', ...(date.getFullYear() !== new Date().getFullYear() ? { year: 'numeric' } : {}) });
  return `${day}, ${date.toLocaleTimeString(intlLocale, { hour: '2-digit', minute: '2-digit' })}`;
}
export function staleness(saved: SavedScan, drive: Drive | undefined, now = Date.now()): { old: boolean; freeDelta: number | null } {
  const delta = drive && saved.driveFree != null ? drive.free - saved.driveFree : null;
  return {
    old: now - saved.scannedAt > 7 * 86400000,
    freeDelta: delta !== null && Math.abs(delta) >= Math.max(1024 ** 3, (drive?.total ?? 0) * .01) ? delta : null,
  };
}
export function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return `0 ${t('units.B')}`;
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value.toLocaleString(intlLocale, { maximumFractionDigits: index === 0 || value >= 100 ? 0 : 1 })} ${t(`units.${units[index]}`)}`;
}
export function formatDelta(bytes: number): string {
  return `${bytes > 0 ? '+' : bytes < 0 ? '−' : ''}${formatSize(Math.abs(bytes))}`;
}
export function notFoundByScan(total: number, free: number, scanned: number): number | null {
  const gap = total - free - scanned;
  return gap >= Math.max(1024 ** 3, total * .01) ? gap : null;
}
export function categoryFor(name: string): string {
  const dot = name.lastIndexOf('.');
  const extension = dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
  return categories.find(c => c.extensions.includes(extension))?.id ?? 'other';
}
export function categoryColor(id?: string): string { return categories.find(c => c.id === id)?.color ?? '#94a3b5'; }
export function percent(value: number, total: number): string {
  if (!Number.isFinite(value) || !Number.isFinite(total) || value <= 0 || total <= 0) return new Intl.NumberFormat(intlLocale, { style: 'percent' }).format(0);
  const share = value / total * 100;
  return (share < .1 ? '<' : '') + new Intl.NumberFormat(intlLocale, { style: 'percent', maximumFractionDigits: 1 }).format(share < .1 ? .001 : value / total);
}
export function middlePath(path: string, limit = 100): string { return path.length > limit ? `${path.slice(0, Math.floor(limit * .4))}…${path.slice(-Math.floor(limit * .6))}` : path; }

export interface ExtensionTotal { extension: string; category: string; bytes: number; allocated: number; files: number }
export type FileSort = 'size' | 'name' | 'modified';
export interface FilePage { total: number; files: LargeFile[] }
export function formatDate(ms: number): string {
  return ms ? new Date(ms).toLocaleDateString(intlLocale, { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' }) : '';
}
export const ageBuckets = [
  { days: 30, id: 'month', color: '#3fb68b' },
  { days: 180, id: 'sixMonths', color: '#8cc152' },
  { days: 365, id: 'year', color: '#e6c34a' },
  { days: 730, id: 'twoYears', color: '#f08c3c' },
  { days: 1825, id: 'fiveYears', color: '#e0533d' },
  { days: Infinity, id: 'older', color: '#a33a6b' },
  { days: 0, id: 'unknown', color: '#94a3b5' },
];
export function ageColor(modified: number, now = Date.now()): string {
  if (!modified) return ageBuckets[6].color;
  return ageBuckets.find(bucket => now - modified < bucket.days * 86400000)!.color;
}
