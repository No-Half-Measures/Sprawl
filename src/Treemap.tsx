import { useEffect, useMemo, useRef, useState } from 'react';
import { categoryColor, count, formatSize, percent, type TreeNode } from './model';
import { fitLabel, layoutTree, textColor, wrapLabel, type Tile } from './layout';

interface Props {
  tree: TreeNode;
  hover: number | null;
  filter: string | null;
  onHover: (id: number | null) => void;
  onOpen: (node: TreeNode) => void;
  onContext: (node: TreeNode, x: number, y: number) => void;
}

export default function Treemap({ tree, hover, filter, onHover, onOpen, onContext }: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0, dpr: window.devicePixelRatio });
  const tiles = useMemo(() => layoutTree(tree, size.width, size.height), [tree, size.width, size.height]);
  const [themeVersion, setThemeVersion] = useState(0);
  const [tip, setTip] = useState<{
    tile: Tile;
    x: number;
    y: number;
  } | null>(null);
  useEffect(() => {
    setTip(null);
  }, [tree]);
  useEffect(() => {
    const theme = window.matchMedia('(prefers-color-scheme: light)');
    const redraw = () => setThemeVersion(version => version + 1);
    theme.addEventListener('change', redraw);
    return () => theme.removeEventListener('change', redraw);
  }, []);
  useEffect(() => {
    const clear = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setTip(null);
    };
    window.addEventListener('keydown', clear);
    return () => window.removeEventListener('keydown', clear);
  }, []);
  useEffect(() => {
    const element = canvas.current!;
    const resize = () => {
      const rect = element.getBoundingClientRect();
      setSize({ width: rect.width, height: rect.height, dpr: window.devicePixelRatio });
    };
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    let media = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    const dprChanged = () => {
      media.removeEventListener('change', dprChanged);
      resize();
      media = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      media.addEventListener('change', dprChanged);
    };
    media.addEventListener('change', dprChanged);
    window.addEventListener('resize', resize);
    resize();
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', resize);
      media.removeEventListener('change', dprChanged);
    };
  }, []);
  useEffect(() => {
    const element = canvas.current!;
    element.width = Math.round(size.width * size.dpr);
    element.height = Math.round(size.height * size.dpr);
    const ctx = element.getContext('2d');
    if (!ctx) return;
    ctx.scale(size.dpr, size.dpr);
    ctx.clearRect(0, 0, size.width, size.height);
    const styles = getComputedStyle(element);
    for (const tile of tiles) {
      const { x, y, width, height, node, frame } = tile;
      const color = categoryColor(node.category);
      ctx.globalAlpha = filter && !frame && node.category !== filter ? .16 : 1;
      ctx.fillStyle = frame ? styles.getPropertyValue('--folder').trim() : color;
      ctx.fillRect(x, y, width, height);
      if (!frame) {
        const gradient = ctx.createLinearGradient(x, y, x + width, y + height);
        gradient.addColorStop(0, 'rgba(255,255,255,.12)');
        gradient.addColorStop(.45, 'rgba(255,255,255,0)');
        gradient.addColorStop(1, 'rgba(0,0,0,.12)');
        ctx.fillStyle = gradient;
        ctx.fillRect(x + 1, y + 1, width - 2, height - 2);
      }
      if (frame ? width > 40 && height > 19 : width >= 72 && height >= 44) {
        const prefix = node.kind === 'dir' ? '▱ ' : '';
        ctx.font = `${frame ? 600 : 500} 12px "Segoe UI", sans-serif`;
        const labelWidth = width - 16 - ctx.measureText(prefix).width;
        const measure = (text: string) => ctx.measureText(text).width;
        const fitted = frame ? fitLabel(node.name, labelWidth, measure) : null;
        const lines = frame ? (fitted ? [fitted] : []) : wrapLabel(node.name, labelWidth, height >= 62 ? 2 : 1, measure);
        ctx.font = '11px "Segoe UI", sans-serif';
        const sizeLabel = formatSize(node.size);
        if (!lines.length || (!frame && ctx.measureText(sizeLabel).width > width - 14)) continue;
        ctx.save();
        ctx.beginPath();
        ctx.rect(x + 5, y + 2, width - 10, height - 4);
        ctx.clip();
        ctx.fillStyle = frame ? styles.getPropertyValue('--text').trim() : textColor(color);
        ctx.font = `${frame ? 600 : 500} 12px "Segoe UI", sans-serif`;
        lines.forEach((line, i) => ctx.fillText(`${i === 0 ? prefix : ''}${line}`, x + 7, y + 15 + i * 15));
        if (!frame) {
          ctx.font = '11px "Segoe UI", sans-serif';
          ctx.fillText(sizeLabel, x + 7, y + 16 + lines.length * 15);
        }
        ctx.restore();
      }
    }
    ctx.globalAlpha = 1;
    for (const tile of tiles) {
      if (hover !== null && (tile.node.id === hover || tile.branch === hover)) {
        ctx.strokeStyle = styles.getPropertyValue('--text').trim();
        ctx.lineWidth = tile.node.id === hover ? 2 : .7;
        ctx.strokeRect(tile.x + 1, tile.y + 1, tile.width - 2, tile.height - 2);
      }
    }
  }, [tiles, size, hover, filter, themeVersion]);
  const hit = (event: React.MouseEvent) => {
    const rect = canvas.current!.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    for (let i = tiles.length - 1; i >= 0; i--) {
      const tile = tiles[i];
      if (x >= tile.x && x <= tile.x + tile.width && y >= tile.y && y <= tile.y + tile.height) {
        return tile;
      }
    }
  };
  return (
    <div className="map-surface">
      <canvas
        ref={canvas}
        aria-label="Disk usage treemap. Use the contents list to navigate with the keyboard."
        onMouseMove={e => {
          const tile = hit(e);
          onHover(tile?.node.id ?? null);
          setTip(tile ? { tile, x: e.nativeEvent.offsetX, y: e.nativeEvent.offsetY } : null);
        }}
        onMouseLeave={() => {
          onHover(null);
          setTip(null);
        }}
        onClick={e => {
          const tile = hit(e);
          if (tile) onOpen(tile.node);
        }}
        onContextMenu={e => {
          e.preventDefault();
          const tile = hit(e);
          if (tile && tile.node.id !== null) onContext(tile.node, e.clientX, e.clientY);
        }}
      />
      {tip && (
        <div
          className="tooltip"
          style={{
            left: Math.max(4, Math.min(tip.x + 14, size.width - 265)),
            top: Math.max(4, Math.min(tip.y + 14, size.height - 105)),
          }}
        >
          <strong>{tip.tile.node.name}</strong>
          <span>{formatSize(tip.tile.node.size)} · {percent(tip.tile.node.size, tree.size)}</span>
          <small>
            {count(tip.tile.node.files, 'file')} · {count(tip.tile.node.folders, 'folder')}
          </small>
          {tip.tile.node.id === null && (
            <small>Zoom into the containing folder for more detail.</small>
          )}
        </div>
      )}
    </div>
  );
}
