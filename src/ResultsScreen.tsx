import Treemap from './Treemap';
import {
  categories,
  categoryColor,
  formatSize,
  percent,
  type Breakdown,
  type Drive,
  type LargeFile,
  type TreeNode,
} from './model';

interface Props {
  current: TreeNode;
  trail: TreeNode[];
  history: TreeNode[][];
  parentSize: number | undefined;
  parentName: string | undefined;
  drive?: Drive;
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
  const visibleBreakdown = breakdown.filter(b => b.bytes > 0);
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
            <strong>{formatSize(current.size)}</strong>
            <span>{current.files.toLocaleString()} files</span>
            <span>{current.folders.toLocaleString()} folders</span>
            {drive ? (
              <span className="drive-capacity">
                <span>{drive.mount}</span>
                <span>Used <strong>{formatSize(Math.max(0, drive.total - drive.free))}</strong></span>
                <span>Free <strong>{formatSize(drive.free)}</strong></span>
                <span>Total <strong>{formatSize(drive.total)}</strong></span>
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
          {skipped.toLocaleString()} items couldn’t be read — run as administrator to include them.
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
        <aside className="details-panel">
          <section className="contents-panel">
            <div className="section-heading">
              <h2>Contents</h2>
              <span>Largest first</span>
            </div>
            <div className="contents-scroll">
              {!loading && tree && tree.id === current.id && tree.children?.map((n, i) => (
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
                        ? `${n.files.toLocaleString()} files`
                        : categories.find(c => c.id === n.category)?.label}
                    >
                      {n.kind === 'dir' || n.id === null
                        ? `${n.files.toLocaleString()} files`
                        : categories.find(c => c.id === n.category)?.label}
                    </span>
                    <strong className="row-size">{formatSize(n.size)}</strong>
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
              ))}
              {!loading && !tree?.children?.length && (
                <p className="quiet-empty">No items in this folder.</p>
              )}
            </div>
          </section>
          <section className="largest-panel">
            <div className="section-heading">
              <h2>Largest files</h2>
              <span>Top {largest.length} in this folder</span>
            </div>
            <div className="largest-scroll">
              {!loading && largest.map((f, i) => (
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
                >
                  <span className="rank">{i + 1}</span>
                  <span className="large-name">
                    <strong title={f.name}>{f.name}</strong>
                    <small title={f.relativePath}>{f.relativePath}</small>
                  </span>
                  <span className="large-size"><strong>{formatSize(f.size)}</strong><small>{percent(f.size, current.size)}</small></span>
                </button>
              ))}
              {!loading && !largest.length && (
                <p className="quiet-empty">No files found.</p>
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
              title={`${formatSize(b.bytes)} · ${b.files.toLocaleString()} files`}
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
                title={`${formatSize(b.bytes)} · ${b.files.toLocaleString()} files`}
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
