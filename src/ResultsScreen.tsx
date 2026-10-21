import { useState, type CSSProperties } from 'react';
import Treemap from './Treemap';
import {
  categories,
  categoryColor,
  count,
  formatAge,
  formatDelta,
  formatScanDate,
  formatSize,
  percent,
  notFoundByScan,
  staleness,
  type Breakdown,
  type Changes,
  type Drive,
  type LargeFile,
  type SavedScan,
  type TreeNode,
} from './model';

export function SnapshotBanner({ saved, drive, rescan, now = Date.now() }: { saved: SavedScan; drive?: Drive; rescan: () => void; now?: number }) {
  const { old, freeDelta } = staleness(saved, drive, now);
  return (
    <div role="status" className={`banner snapshot ${old || freeDelta !== null ? 'warning' : ''}`}>
      <span>
        Snapshot from {formatScanDate(saved.scannedAt)} · {formatAge(saved.scannedAt, now)}
        {old && ' · This may be out of date.'}
        {freeDelta !== null && ` · Free space has changed by ${formatSize(Math.abs(freeDelta))} since this scan.`}
      </span>
      <button onClick={rescan}>Rescan</button>
    </div>
  );
}

interface Props {
  current: TreeNode;
  trail: TreeNode[];
  history: TreeNode[][];
  parentSize: number | undefined;
  parentName: string | undefined;
  drive?: Drive;
  snapshot?: SavedScan | null;
  recycled?: number;
  deleted?: number;
  elevated?: boolean;
  changes?: Changes | null;
  scanPath: string;
  skipped: number;
  error: string;
  loading: boolean;
  tree: TreeNode | null;
  hover: number | null;
  linkedHover: number | null;
  filter: string | null;
  largest: LargeFile[];
  breakdown: Breakdown[];
  back: () => void;
  up: () => void;
  navigate: (next: TreeNode[]) => void;
  enter: (node: TreeNode) => void;
  setHover: (id: number | null) => void;
  setFilter: (category: string | null) => void;
  context: (node: TreeNode, x: number, y: number) => void;
  action: (command: string, node: TreeNode) => Promise<void>;
}

export default function ResultsScreen({
  current,
  trail,
  history,
  parentSize,
  parentName,
  drive,
  snapshot,
  recycled = snapshot?.recycled ?? 0,
  deleted = snapshot?.deleted ?? 0,
  elevated,
  changes,
  scanPath,
  skipped,
  error,
  loading,
  tree,
  hover,
  linkedHover,
  filter,
  largest,
  breakdown,
  back,
  up,
  navigate,
  enter,
  setHover,
  setFilter,
  context,
  action,
}: Props) {
  const [fileTab, setFileTab] = useState<'largest' | 'grown'>('largest');
  const [split, setSplit] = useState(() => {
    try {
      const saved = localStorage.getItem('diskmapper.split');
      const value = saved?.trim() ? Number(saved) : NaN;
      return Number.isFinite(value) ? Math.max(20, Math.min(80, value)) : 48;
    } catch { return 48; }
  });
  const [dragging, setDragging] = useState(false);
  const updateSplit = (value: number, persist = false) => {
    const next = Math.max(20, Math.min(80, value));
    setSplit(next);
    if (persist) {
      try { localStorage.setItem('diskmapper.split', String(next)); } catch {}
    }
  };
  const visibleBreakdown = breakdown.filter(b => b.bytes > 0);
  const total = snapshot?.driveTotal ?? drive?.total ?? 0;
  const free = snapshot?.driveFree ?? drive?.free ?? 0;
  const used = Math.max(0, total - free);
  const notFound = drive ? notFoundByScan(total, free, current.size + recycled + deleted) : null;
  const notFoundText = (skipped > 0
    ? `Includes ${count(skipped, 'item')} that couldn't be read, plus system files and file-system overhead.`
    : "System files, restore points, and file-system overhead the scan can't see.")
    + (elevated === false ? ' Run as administrator to include more.' : '');
  const previousSizes = new Map(changes?.children.map(n => [n.id, n.previousSize]));
  const since = changes ? formatScanDate(changes.since) : '';
  const showGrown = fileTab === 'grown' && !!changes;
  const files = showGrown ? changes.grown : largest;
  const currentPath = trail.length > 1
    ? `${scanPath.replace(/[\\/]$/, '')}\\${trail.slice(1).map(n => n.name).join('\\')}`
    : scanPath;
  return (
    <>
      <section className="results-header">
        <nav aria-label="Folder navigation">
          <button
            title="Back (Backspace)"
            aria-label="Back (Backspace)"
            disabled={!history.length}
            onClick={back}
          >
            ← Back
          </button>
          <button
            title="Up one folder (Alt+↑)"
            aria-label="Up one folder (Alt+↑)"
            disabled={trail.length < 2}
            onClick={up}
          >
            ↑ Up one folder
          </button>
          <div className="breadcrumbs">
            {trail.map((n, i) => (
              <span key={n.id}>
                {i > 0 && (
                  <b>›</b>
                )}
                <button
                  title={i === 0 ? scanPath : n.name}
                  aria-current={i === trail.length - 1 ? 'location' : undefined}
                  onClick={() => i < trail.length - 1 && navigate(trail.slice(0, i + 1))}
                >
                  {n.name}
                </button>
              </span>
            ))}
          </div>
        </nav>
        <div className="folder-summary">
          <h1 title={currentPath}>{current.name}</h1>
          <span>
            {drive ? (
              <span title="Total size of files the scan could read. Used also counts unreadable items and file-system overhead.">Scanned <strong>{formatSize(current.size)}</strong></span>
            ) : <strong>{formatSize(current.size)}</strong>}
            {drive && recycled > 0 && <span title="Moved from this snapshot. Space is freed when you empty the Recycle Bin.">· {formatSize(recycled)} in Recycle Bin</span>}
            <span>{count(current.files, 'file')}</span>
            <span>{count(current.folders, 'folder')}</span>
            {changes && (
              <span>{changes.previousSize === null ? 'New' : current.size === changes.previousSize ? 'No change' : formatDelta(current.size - changes.previousSize)} since {since}</span>
            )}
            {drive ? (
              <span className="drive-capacity">
                <span>{drive.mount}</span>
                <span>Used <strong>{formatSize(used)}</strong></span>
                <span>Free <strong>{formatSize(free)}</strong></span>
                <span>Total <strong>{formatSize(total)}</strong></span>
                {notFound !== null && <span title={notFoundText}>Not found <strong>{formatSize(notFound)}</strong></span>}
              </span>
            ) : parentSize !== undefined && (
              <span>{percent(current.size, parentSize)} of {parentName}</span>
            )}
          </span>
        </div>
      </section>
      {skipped > 0 && (
        <div
          className="skipped"
          role="status"
        >
          {count(skipped, 'item')} couldn’t be read — run as administrator to include them.
        </div>
      )}
      <div
        className={`workspace ${loading ? 'loading' : ''}`}
        aria-busy={loading}
      >
        <section className="map-panel">
          <div className="section-heading">
            <h2>Space map</h2>
            <span>{loading ? 'Loading folder…' : 'Bigger blocks use more space · Colors show file type · Click a folder to zoom in'}</span>
          </div>
          {tree && !loading && tree.id === current.id && tree.size > 0 ? (
            <Treemap
              tree={tree}
              hover={hover}
              filter={filter}
              onHover={setHover}
              onOpen={enter}
              onContext={context}
            />
          ) : (
            <div className="empty-state">
              <h3>
                {loading
                  ? 'Opening folder…'
                  : error
                    ? 'Folder unavailable'
                    : current.files + current.folders === 0
                      ? 'This folder is empty'
                      : 'No file data to map'}
              </h3>
              <p>
                {loading
                  ? 'Preparing the map and file details.'
                  : error
                    ? 'Rescan the folder to try again.'
                    : 'Zero-byte files and empty folders appear in Contents.'}
              </p>
            </div>
          )}
        </section>
        <aside className="details-panel" style={{ '--contents': `${split}fr`, '--largest': `${100 - split}fr` } as CSSProperties}>
          <section className="contents-panel">
            <div className="section-heading">
              <h2>Contents</h2>
              <span>Largest first</span>
            </div>
            <div className="contents-scroll">
              {!loading && tree && tree.id === current.id && tree.children?.map((n, i) => {
                const previous = n.id === null ? undefined : previousSizes.get(n.id);
                const delta = n.size - (previous ?? 0);
                return (
                <button
                  key={n.id ?? `other-${i}`}
                  title={n.name}
                  className={`content-row ${linkedHover !== null && linkedHover === n.id ? 'linked-hover' : ''}`}
                  onClick={() => enter(n)}
                  onMouseEnter={() => setHover(n.id)}
                  onMouseLeave={() => setHover(null)}
                  onFocus={() => setHover(n.id)}
                  onBlur={() => setHover(null)}
                  onContextMenu={e => {
                    e.preventDefault();
                    context(n, e.clientX, e.clientY);
                  }}
                  onKeyDown={e => {
                    if (e.key === 'Delete' && n.id !== null && n.id !== 0) {
                      e.preventDefault();
                      void action('delete', n);
                    }
                    if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
                      e.preventDefault();
                      const r = e.currentTarget.getBoundingClientRect();
                      context(n, r.left, r.top);
                    }
                  }}
                >
                  <span className="row-name">
                    <span
                      className="file-symbol"
                      style={{ color: n.kind === 'dir' ? undefined : categoryColor(n.category) }}
                    >
                      {n.kind === 'dir' ? '▱' : n.id === null ? '⋯' : '▪'}
                    </span>
                    <span title={n.name}>{n.name}</span>
                    {n.kind === 'dir' && (
                      <span className="chevron">›</span>
                    )}
                  </span>
                  <span className="row-data">
                    <span
                      title={n.kind === 'dir' || n.id === null
                        ? count(n.files, 'file')
                        : categories.find(c => c.id === n.category)?.label}
                    >
                      {n.kind === 'dir' || n.id === null
                        ? count(n.files, 'file')
                        : categories.find(c => c.id === n.category)?.label}
                    </span>
                    <strong className="row-size">
                      {previous !== undefined && (previous === null || Math.abs(delta) >= 1024 ** 2) && (
                        <small className={`delta ${delta < 0 ? 'shrink' : 'growth'}`}>{previous === null ? 'New' : formatDelta(delta)}</small>
                      )}
                      {formatSize(n.size)}
                    </strong>
                    <span className="row-percent">{percent(n.size, current.size)}</span>
                  </span>
                  <span className="row-meter">
                    <i
                      style={{
                        width: `${current.size ? n.size / current.size * 100 : 0}%`,
                        background: n.kind === 'dir' ? 'var(--accent)' : categoryColor(n.category),
                      }}
                    />
                  </span>
                </button>
                );
              })}
              {!loading && !tree?.children?.length && (
                <p className="quiet-empty">No items in this folder.</p>
              )}
              {!loading && changes && changes.removed.items > 0 && (
                <div className="content-row summary-row">
                  <span>{count(changes.removed.items, 'removed item')}</span>
                  <strong className="delta shrink">−{formatSize(changes.removed.bytes)}</strong>
                </div>
              )}
              {!loading && notFound !== null && (
                <div className="content-row not-found-row">
                  <span>Not found by scan</span>
                  <small title={notFoundText}>{notFoundText}</small>
                  <span className="row-data"><span /><strong>{formatSize(notFound)}</strong><span className="row-percent">{percent(notFound, used)}</span></span>
                  <span className="row-meter"><i style={{ width: `${used ? notFound / used * 100 : 0}%` }} /></span>
                </div>
              )}
            </div>
          </section>
          <div
            className={`panel-divider${dragging ? ' dragging' : ''}`}
            role="separator"
            aria-orientation="horizontal"
            aria-label="Resize contents and largest files"
            aria-valuemin={20}
            aria-valuemax={80}
            aria-valuenow={Math.round(split)}
            tabIndex={0}
            onPointerDown={e => {
              if (e.button !== 0 || dragging) return;
              e.preventDefault();
              e.currentTarget.focus();
              e.currentTarget.setPointerCapture(e.pointerId);
              setDragging(true);
            }}
            onPointerMove={e => {
              if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
              const rect = e.currentTarget.parentElement!.getBoundingClientRect();
              if (rect.height > 0) updateSplit((e.clientY - rect.top) / rect.height * 100);
            }}
            onPointerUp={e => {
              if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
              e.currentTarget.releasePointerCapture(e.pointerId);
              setDragging(false);
              updateSplit(split, true);
            }}
            onPointerCancel={() => {
              setDragging(false);
              updateSplit(split, true);
            }}
            onLostPointerCapture={() => setDragging(false)}
            onKeyDown={e => {
              const next = e.key === 'ArrowUp' ? split - 5 : e.key === 'ArrowDown' ? split + 5 : e.key === 'Home' ? 20 : e.key === 'End' ? 80 : null;
              if (next === null) return;
              e.preventDefault();
              updateSplit(next, true);
            }}
            onDoubleClick={() => updateSplit(48, true)}
          />
          <section className="largest-panel">
            <div className="section-heading">
              {changes ? (
                <div className="file-tabs" role="tablist" aria-label="File ranking">
                  <button role="tab" aria-selected={!showGrown} onClick={() => setFileTab('largest')}>Largest files</button>
                  <button role="tab" aria-selected={showGrown} onClick={() => setFileTab('grown')}>Grown since {since}</button>
                </div>
              ) : <h2>Largest files</h2>}
              <span>Top {files.length} in this folder</span>
            </div>
            <div className="largest-scroll">
              {!loading && files.map((f, i) => (
                <button
                  key={f.id}
                  className="large-row"
                  title={`${f.relativePath}\nReveal in Explorer`}
                  onClick={() => void action('reveal_in_explorer', { ...f, kind: 'file', files: 1, folders: 0 })}
                  onMouseEnter={() => setHover(f.id)}
                  onMouseLeave={() => setHover(null)}
                  onContextMenu={e => {
                    e.preventDefault();
                    context({ ...f, kind: 'file', files: 1, folders: 0 }, e.clientX, e.clientY);
                  }}
                  onKeyDown={e => {
                    if (e.key === 'Delete') {
                      e.preventDefault();
                      void action('delete', { ...f, kind: 'file', files: 1, folders: 0 });
                    }
                    if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
                      e.preventDefault();
                      const r = e.currentTarget.getBoundingClientRect();
                      context({ ...f, kind: 'file', files: 1, folders: 0 }, r.left, r.top);
                    }
                  }}
                >
                  <span className="rank">{i + 1}</span>
                  <span className="large-name">
                    <strong title={f.name}>{f.name}</strong>
                    <small title={f.relativePath}>{f.relativePath}</small>
                  </span>
                  {showGrown && 'previousSize' in f ? (
                    <span className="large-size"><strong className="delta growth">{formatDelta(f.size - (f.previousSize ?? 0))}</strong><small>{f.previousSize === null ? 'New' : `was ${formatSize(f.previousSize)}`}</small></span>
                  ) : <span className="large-size"><strong>{formatSize(f.size)}</strong><small>{percent(f.size, current.size)}</small></span>}
                </button>
              ))}
              {!loading && !files.length && (
                <p className="quiet-empty">{showGrown ? `No files grew since ${since}.` : 'No files found.'}</p>
              )}
            </div>
          </section>
        </aside>
      </div>
      <footer className="type-panel">
        <div
          className="type-bar"
          aria-label="Space used by file category"
        >
          {visibleBreakdown.map(b => (
            <button
              key={b.category}
              aria-label={`${categories.find(c => c.id === b.category)?.label}: ${formatSize(b.bytes)}`}
              title={`${formatSize(b.bytes)} · ${count(b.files, 'file')}`}
              style={{
                width: `${current.size ? b.bytes / current.size * 100 : 0}%`,
                background: categoryColor(b.category),
                opacity: filter && filter !== b.category ? .25 : 1,
              }}
              onMouseEnter={() => setFilter(b.category)}
              onMouseLeave={() => setFilter(null)}
              onFocus={() => setFilter(b.category)}
              onBlur={() => setFilter(null)}
            />
          ))}
        </div>
        <div className="type-legend">
          {visibleBreakdown.map(b => {
            const c = categories.find(c => c.id === b.category);
            return (
              <button
                key={b.category}
                onMouseEnter={() => setFilter(b.category)}
                onMouseLeave={() => setFilter(null)}
                onFocus={() => setFilter(b.category)}
                onBlur={() => setFilter(null)}
                style={{ opacity: filter && filter !== b.category ? .4 : 1 }}
                title={`${formatSize(b.bytes)} · ${count(b.files, 'file')}`}
              >
                <i style={{ background: categoryColor(b.category) }} />
                {c?.label ?? b.category}
                <span>{formatSize(b.bytes)} · {percent(b.bytes, current.size)}</span>
              </button>
            );
          })}
        </div>
      </footer>
    </>
  );
}
