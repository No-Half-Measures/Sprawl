import { describe, expect, it } from 'vitest';
import { categories, categoryFor, formatSize, percent, type TreeNode } from './model';
import { fitLabel, layoutTree, textColor } from './layout';

describe('binary size formatting', () => {
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
  it('omits names when fewer than four characters can be shown before the ellipsis', () => {
    const measure = (text: string) => Array.from(text).length * 7;
    expect(fitLabel('long-filename.lib', 28, measure)).toBeNull();
    expect(fitLabel('long-filename.lib', 35, measure)).toBe('long…');
    expect(fitLabel('file.lib', 70, measure)).toBe('file.lib');
    expect(fitLabel('file.lib', 0, measure)).toBeNull();
  });
});
