import categories from './categories.json';
export { categories };
export interface TreeNode { id: number | null; name: string; kind: 'dir' | 'file' | 'other'; size: number; files: number; folders: number; category?: string; children?: TreeNode[] }
export interface LargeFile { id: number; name: string; relativePath: string; size: number; category: string }
export interface GrownFile { id: number; name: string; relativePath: string; size: number; previousSize: number | null }
export interface Changes { since: number; previousSize: number | null; children: { id: number; previousSize: number | null }[]; removed: { items: number; bytes: number }; grown: GrownFile[] }
export interface Breakdown { category: string; bytes: number; files: number }
export interface Drive { mount: string; label: string; total: number; free: number }
export interface AdminStatus { supported: boolean; elevated: boolean; alwaysElevate: boolean; declined: boolean }
export interface SavedScan { version: number; path: string; scannedAt: number; size: number; files: number; folders: number; errors: number; driveFree: number | null; driveTotal: number | null }
export interface Progress { files: number; folders: number; bytes: number; errors: number; currentPath: string }
export const emptyProgress: Progress = { files: 0, folders: 0, bytes: 0, errors: 0, currentPath: '' };
export function isDriveRoot(path: string, mount: string): boolean {
  return path.replace(/[\\/]+$/, '').toLowerCase() === mount.replace(/[\\/]+$/, '').toLowerCase();
}
export function formatAge(ms: number, now = Date.now()): string {
  const minutes = Math.floor(Math.max(0, now - ms) / 60000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'} ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? '' : 's'} ago`;
  const days = Math.floor(hours / 24);
  if (days === 1) return 'yesterday';
  if (days < 7) return `${days} days ago`;
  if (days < 30) { const weeks = Math.floor(days / 7); return `${weeks} week${weeks === 1 ? '' : 's'} ago`; }
  if (days < 365) { const months = Math.floor(days / 30); return `${months} month${months === 1 ? '' : 's'} ago`; }
  const years = Math.floor(days / 365);
  return `${years} year${years === 1 ? '' : 's'} ago`;
}
export function formatScanDate(ms: number): string {
  const date = new Date(ms);
  const day = date.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', ...(date.getFullYear() !== new Date().getFullYear() ? { year: 'numeric' } : {}) });
  return `${day}, ${date.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })}`;
}
export function staleness(saved: SavedScan, drive: Drive | undefined, now = Date.now()): { old: boolean; freeDelta: number | null } {
  const delta = drive && saved.driveFree != null ? drive.free - saved.driveFree : null;
  return {
    old: now - saved.scannedAt > 7 * 86400000,
    freeDelta: delta !== null && Math.abs(delta) >= Math.max(1024 ** 3, (drive?.total ?? 0) * .01) ? delta : null,
  };
}
export function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value.toLocaleString('en-GB', { maximumFractionDigits: index === 0 || value >= 100 ? 0 : 1 })} ${units[index]}`;
}
export function formatDelta(bytes: number): string {
  return `${bytes > 0 ? '+' : bytes < 0 ? '−' : ''}${formatSize(Math.abs(bytes))}`;
}
export function categoryFor(name: string): string {
  const dot = name.lastIndexOf('.');
  const extension = dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
  return categories.find(c => c.extensions.includes(extension))?.id ?? 'other';
}
export function categoryColor(id?: string): string { return categories.find(c => c.id === id)?.color ?? '#94a3b5'; }
export function percent(value: number, total: number): string {
  if (!Number.isFinite(value) || !Number.isFinite(total) || value <= 0 || total <= 0) return '0%';
  const share = value / total * 100;
  return share < .1 ? '<0.1%' : `${share.toLocaleString('en-GB', { maximumFractionDigits: 1 })}%`;
}
export function middlePath(path: string, limit = 100): string { return path.length > limit ? `${path.slice(0, Math.floor(limit * .4))}…${path.slice(-Math.floor(limit * .6))}` : path; }
