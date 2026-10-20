import categories from './categories.json';
export { categories };
export interface TreeNode { id: number | null; name: string; kind: 'dir' | 'file' | 'other'; size: number; files: number; folders: number; category?: string; children?: TreeNode[] }
export interface LargeFile { id: number; name: string; relativePath: string; size: number; category: string }
export interface Breakdown { category: string; bytes: number; files: number }
export interface Drive { mount: string; label: string; total: number; free: number }
export interface Progress { files: number; folders: number; bytes: number; errors: number; currentPath: string }
export const emptyProgress: Progress = { files: 0, folders: 0, bytes: 0, errors: 0, currentPath: '' };
export function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value.toLocaleString('en-GB', { maximumFractionDigits: index === 0 || value >= 100 ? 0 : 1 })} ${units[index]}`;
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
