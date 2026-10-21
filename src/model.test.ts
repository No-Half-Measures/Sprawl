import { afterEach, describe, expect, it, vi } from 'vitest';
import { categories, categoryFor, formatAge, formatDelta, formatScanDate, formatSize, isDriveRoot, percent, staleness, type SavedScan, type TreeNode } from './model';
import { fitLabel, layoutTree, textColor, wrapLabel } from './layout';

afterEach(() => vi.useRealTimers());

describe('saved scan dates and freshness', () => {
  const now = new Date(2026, 9, 20, 15, 0).getTime();
  const scan: SavedScan = { version: 1, path: 'C:\\', scannedAt: now, size: 1, files: 1, folders: 0, errors: 0, driveFree: 100 * 1024 ** 3, driveTotal: 500 * 1024 ** 3 };
  const drive = { mount: 'C:\\', label: 'Disk', free: 100 * 1024 ** 3, total: 500 * 1024 ** 3 };
  it('formats relative ages at unit boundaries', () => {
    for (const [age, text] of [
      [-1, 'just now'], [0, 'just now'], [59999, 'just now'],
      [60000, '1 minute ago'], [300000, '5 minutes ago'], [3599999, '59 minutes ago'],
      [3600000, '1 hour ago'], [10800000, '3 hours ago'], [86399999, '23 hours ago'],
      [86400000, 'yesterday'], [3 * 86400000, '3 days ago'],
      [7 * 86400000, '1 week ago'], [14 * 86400000, '2 weeks ago'],
      [30 * 86400000, '1 month ago'], [90 * 86400000, '3 months ago'],
      [365 * 86400000, '1 year ago'], [730 * 86400000, '2 years ago'],
    ] as const) expect(formatAge(now - age, now)).toBe(text);
  });
  it('formats a local en-GB date and includes the year only for other years', () => {
    vi.useFakeTimers();
    vi.setSystemTime(now);
    expect(formatScanDate(new Date(2026, 9, 13, 14, 2).getTime())).toBe('13 Oct, 14:02');
    expect(formatScanDate(new Date(2025, 9, 13, 14, 2).getTime())).toBe('13 Oct 2025, 14:02');
  });
  it('warns only beyond seven days', () => {
    expect(staleness(scan, drive, now + 7 * 86400000)).toEqual({ old: false, freeDelta: null });
    expect(staleness(scan, drive, now + 7 * 86400000 + 1)).toEqual({ old: true, freeDelta: null });
  });
  it('uses the larger of 1 GiB and 1% of current drive capacity, in either direction', () => {
    const threshold = drive.total * .01;
    expect(staleness(scan, { ...drive, free: drive.free + threshold - 1 }, now).freeDelta).toBeNull();
    expect(staleness(scan, { ...drive, free: drive.free + threshold }, now).freeDelta).toBe(threshold);
    expect(staleness(scan, { ...drive, free: drive.free - threshold }, now).freeDelta).toBe(-threshold);
    const small = { ...drive, total: 50 * 1024 ** 3 };
    expect(staleness(scan, { ...small, free: drive.free + 1024 ** 3 - 1 }, now).freeDelta).toBeNull();
    expect(staleness(scan, { ...small, free: drive.free + 1024 ** 3 }, now).freeDelta).toBe(1024 ** 3);
    expect(staleness({ ...scan, driveFree: 0 }, { ...small, free: 1024 ** 3 }, now).freeDelta).toBe(1024 ** 3);
  });
  it('has no space warning without a drive or saved free space', () => {
    expect(staleness(scan, undefined, now).freeDelta).toBeNull();
    expect(staleness({ ...scan, driveFree: null, driveTotal: null }, drive, now).freeDelta).toBeNull();
  });
});

describe('drive root matching', () => {
  it('ignores case and trailing separators without matching folders', () => {
    for (const path of ['C:\\', 'C:', 'c:\\', 'C:/', 'C:\\\\']) {
      expect(isDriveRoot(path, 'C:\\')).toBe(true);
    }
    expect(isDriveRoot('C:\\', 'c:/')).toBe(true);
    expect(isDriveRoot('C:\\Users', 'C:\\')).toBe(false);
    expect(isDriveRoot('E:\\Projects\\Other', 'E:\\')).toBe(false);
    expect(isDriveRoot('D:\\', 'C:\\')).toBe(false);
  });
});

describe('binary size formatting', () => {
  it('formats signed changes with a real minus sign', () => {
    expect(formatDelta(1.5 * 1024 ** 3)).toBe('+1.5 GB');
    expect(formatDelta(-300 * 1024 ** 2)).toBe('−300 MB');
    expect(formatDelta(0)).toBe('0 B');
    expect(formatDelta(-0)).toBe('0 B');
  });
  it('handles zero, boundaries, fractions and large files', () => {
    expect(formatSize(0)).toBe('0 B'); expect(formatSize(-1)).toBe('0 B');
    expect(formatSize(1023)).toBe('1,023 B'); expect(formatSize(1024)).toBe('1 KB');
    expect(formatSize(1536)).toBe('1.5 KB'); expect(formatSize(1024 ** 4)).toBe('1 TB');
    expect(formatSize(Infinity)).toBe('0 B');
  });
});

describe('percentage formatting', () => {
  it('distinguishes tiny nonzero shares from zero and removes trailing decimals', () => {
    expect(percent(0, 100)).toBe('0%');
    expect(percent(10, 0)).toBe('0%');
    expect(percent(1, 10000)).toBe('<0.1%');
    expect(percent(1, 1000)).toBe('0.1%');
    expect(percent(30, 100)).toBe('30%');
    expect(percent(63, 1000)).toBe('6.3%');
    expect(percent(100, 100)).toBe('100%');
    expect(percent(NaN, 100)).toBe('0%');
  });
});
describe('shared file categories', () => {
  it('maps case-insensitive extensions and unknown names', () => {
    expect(categories).toHaveLength(8);
    for (const c of categories) for (const ext of c.extensions) expect(categoryFor(`file.${ext.toUpperCase()}`)).toBe(c.id);
    expect(categoryFor('README')).toBe('other'); expect(categoryFor('.gitignore')).toBe('other'); expect(categoryFor('a.unknown')).toBe('other');
    expect(new Set(categories.map(c => c.color)).size).toBe(8);
    const extensions = categories.flatMap(c => c.extensions); expect(new Set(extensions).size).toBe(extensions.length);
  });
  it('recognizes build outputs, game packages and virtual disk images', () => {
    for (const extension of ['lib','pdb','obj','o','a','rlib','rmeta','pch','ilk','exp','idb','class','jar','wasm','node']) {
      expect(categoryFor(`output.${extension}`)).toBe('code');
    }
    for (const extension of ['pak','pbo','vpk','bundle','assets','uasset','umap','ucas','utoc','bsa','ba2','esm','esp','gcf','unity3d','resource','sav']) {
      expect(categoryFor(`game.${extension}`)).toBe('apps');
    }
    for (const extension of ['vhd','vhdx','vmdk','qcow2','img','wim','esd']) {
      expect(categoryFor(`disk.${extension}`)).toBe('archives');
    }
    expect(categories.find(c => c.id === 'code')?.label).toBe('Code & build');
    expect(categories.find(c => c.id === 'apps')?.label).toBe('Apps & games');
  });
});
const file = (id: number, size: number): TreeNode => ({ id, name: `file${id}`, kind: 'file', size, files: 1, folders: 0, category: 'other' });
const root: TreeNode = { id: 0, name: 'root', kind: 'dir', size: 100, files: 3, folders: 0, children: [file(1, 60), file(2, 30), file(3, 10)] };
describe('squarified treemap', () => {
  it('gives proportional areas without overlap or exceeding bounds', () => {
    const tiles = layoutTree(root, 1000, 600, false);
    expect(tiles).toHaveLength(3);
    for (const t of tiles) {
      expect(t.width * t.height / 600000).toBeCloseTo(t.node.size / 100, 8);
      expect(t.x).toBeGreaterThanOrEqual(0); expect(t.y).toBeGreaterThanOrEqual(0);
      expect(t.x + t.width).toBeLessThanOrEqual(1000.000001); expect(t.y + t.height).toBeLessThanOrEqual(600.000001);
      for (const other of tiles) if (other !== t) {
        const overlap = Math.max(0, Math.min(t.x + t.width, other.x + other.width) - Math.max(t.x, other.x)) * Math.max(0, Math.min(t.y + t.height, other.y + other.height) - Math.max(t.y, other.y));
        expect(overlap).toBeLessThan(.000001);
      }
    }
  });
  it('culls small tiles and handles empty folders and tiny viewports', () => {
    const tree = { ...root, size: 1000001, children: [file(1, 1000000), file(2, 1)] };
    expect(layoutTree(tree, 600, 400).map(t => t.node.id)).toEqual([1]);
    expect(layoutTree({ ...root, size: 0, children: [] }, 600, 400)).toEqual([]);
    expect(layoutTree(root, 0, 400)).toEqual([]); expect(layoutTree(root, 2, 2)).toEqual([]);
  });
  it('contains nested tiles within folder frames with an inset and label strip', () => {
    const folder: TreeNode = { ...root, id: 4, name: 'folder' };
    const tiles = layoutTree({ ...root, children: [folder] }, 800, 600);
    const frame = tiles.find(t => t.node.id === 4)!;
    for (const child of tiles.filter(t => t.depth === 2)) {
      expect(child.y).toBeGreaterThanOrEqual(frame.y + 23); expect(child.x).toBeGreaterThan(frame.x);
      expect(child.x + child.width).toBeLessThan(frame.x + frame.width);
      expect(child.ancestors).toEqual([0, 4, child.node.id]); expect(child.branch).toBe(4);
    }
    expect(textColor('#ffffff')).toBe('#152330'); expect(textColor('#000000')).toBe('#ffffff');
  });
  it('uses readable foregrounds for all eight category colors', () => {
    function luminance(hex: string) {
      const rgb = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
      return rgb[0] * .2126 + rgb[1] * .7152 + rgb[2] * .0722;
    }
    for (const c of categories) {
      const background = luminance(c.color), foreground = luminance(textColor(c.color));
      expect((Math.max(background, foreground) + .05) / (Math.min(background, foreground) + .05)).toBeGreaterThanOrEqual(4.5);
    }
  });
  it('colors depth-limited folders and folders whose children have no drawing room as leaves', () => {
    const folder: TreeNode = { ...root, id: 4, category: 'code', children: undefined };
    const leaf = layoutTree({ ...root, children: [folder] }, 800, 600)[0];
    expect(leaf.frame).toBe(false);
    expect(leaf.node.category).toBe('code');
    const crowded: TreeNode = { ...folder, children: Array.from({length:100}, (_,i) => file(i+5,1)) };
    const tiles = layoutTree({ ...root, children: [crowded] }, 20, 20);
    expect(tiles).toHaveLength(1);
    expect(tiles[0].frame).toBe(false);
    expect(tiles[0].node.category).toBe('code');
  });
  it('truncates names when four characters and an ellipsis fit', () => {
    const measure = (text: string) => Array.from(text).length * 7;
    expect(fitLabel('long-filename.lib', 28, measure)).toBeNull();
    expect(fitLabel('long-filename.lib', 35, measure)).toBe('long…');
    expect(fitLabel('long-filename.lib', 63, measure)).toBe('long-fil…');
    expect(fitLabel('long-filename.lib', 70, measure)).toBe('long-file…');
    expect(fitLabel('😀long-filename.lib', 35, measure)).toBe('😀lon…');
    expect(fitLabel('file.lib', 49, measure)).toBe('file.l…');
    expect(fitLabel('file.lib', 70, measure)).toBe('file.lib');
    expect(fitLabel('file.lib', 0, measure)).toBeNull();
  });
});

describe('tile label wrapping', () => {
  const measure = (text: string) => Array.from(text).length * 7;
  it('keeps names that fit on one line', () => {
    expect(wrapLabel('file.lib', 56, 2, measure)).toEqual(['file.lib']);
  });
  it('breaks after separators that fit on the line', () => {
    for (const separator of [' ', '-', '_', '.', '\\', '/']) {
      expect(wrapLabel(`long${separator}filename`, 63, 2, measure)).toEqual([`long${separator}`, 'filename']);
    }
    expect(wrapLabel('one-two-file', 56, 2, measure)).toEqual(['one-two-', 'file']);
  });
  it('breaks between characters without splitting unicode characters', () => {
    expect(wrapLabel('abcdefghijkl', 42, 2, measure)).toEqual(['abcdef', 'ghijkl']);
    expect(wrapLabel('😀abcdefghij', 42, 2, measure)).toEqual(['😀abcde', 'fghij']);
  });
  it('truncates the final line', () => {
    expect(wrapLabel('long-filename.lib', 56, 2, measure)).toEqual(['long-', 'filenam…']);
  });
  it('hides names when there is too little room', () => {
    expect(wrapLabel('long-filename.lib', 28, 2, measure)).toEqual([]);
    expect(wrapLabel('file.lib', 0, 2, measure)).toEqual([]);
    expect(wrapLabel('file.lib', 56, 0, measure)).toEqual([]);
  });
  it('uses fitLabel for a single line', () => {
    for (const width of [0, 28, 35, 70, 140]) {
      const fitted = fitLabel('long-filename.lib', width, measure);
      expect(wrapLabel('long-filename.lib', width, 1, measure)).toEqual(fitted === null ? [] : [fitted]);
    }
  });
});
