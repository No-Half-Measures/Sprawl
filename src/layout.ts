import { hierarchy, treemap, treemapSquarify } from 'd3-hierarchy';
import type { TreeNode } from './model';
export interface Tile { node: TreeNode; x: number; y: number; width: number; height: number; depth: number; ancestors: (number | null)[]; branch: number | null; frame: boolean }
export function layoutTree(tree: TreeNode, width: number, height: number, frames = true): Tile[] {
  if (width <= 0 || height <= 0 || tree.size <= 0) return [];
  const root = hierarchy(tree).sum(n => n.children?.length ? 0 : n.size).sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
  const layout = treemap<TreeNode>().tile(treemapSquarify).size([width, height]).round(false)
    .paddingInner(frames ? 2 : 0).paddingOuter(n => frames && n.depth > 0 ? 3 : 0)
    .paddingTop(n => frames && n.depth > 0 && (n.value ?? 0) / tree.size * width * height > 3500 ? 23 : frames && n.depth > 0 ? 3 : 0)(root);
  const visible = (node: { x0: number; x1: number; y0: number; y1: number }) => node.x1 - node.x0 >= 3 && node.y1 - node.y0 >= 3;
  return layout.descendants().filter(n => n.depth > 0 && visible(n)).map(n => {
    const ancestors = n.ancestors().reverse();
    return { node: n.data, x: n.x0, y: n.y0, width: n.x1 - n.x0, height: n.y1 - n.y0, depth: n.depth, ancestors: ancestors.map(a => a.data.id), branch: ancestors[1]?.data.id ?? null, frame: !!n.children?.some(visible) };
  });
}

export function fitLabel(name: string, width: number, measure: (text: string) => number): string | null {
  if (width <= 0) return null;
  if (measure(name) <= width) return name;
  const characters = Array.from(name);
  const minimum = Math.max(8, Math.ceil(characters.length / 2));
  while (characters.length > minimum) {
    characters.pop();
    const label = `${characters.join('')}…`;
    if (measure(label) <= width) return label;
  }
  return null;
}
export function textColor(hex: string): string {
  const channels = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
  return channels[0] * .2126 + channels[1] * .7152 + channels[2] * .0722 > .2 ? '#152330' : '#ffffff';
}
