import { formatAge, formatSize, isDriveRoot, middlePath, staleness, type AdminStatus, type Drive, type SavedScan } from './model';

interface Props {
  drives: Drive[];
  ready: boolean;
  opening?: string | null;
  pickFolder: () => Promise<void>;
  start: (path: string) => Promise<void>;
  saved: SavedScan[];
  openSaved: (path: string, scannedAt: number) => Promise<void>;
  clearSaved: () => Promise<void>;
  admin?: AdminStatus | null;
  adminBusy?: boolean;
  restartAsAdmin: () => Promise<void>;
  setAlwaysElevate: (enabled: boolean) => Promise<void>;
  now?: number;
}

export default function HomeScreen({
  drives,
  ready,
  opening = null,
  pickFolder,
  start,
  saved,
  openSaved,
  clearSaved,
  admin,
  adminBusy = false,
  restartAsAdmin,
  setAlwaysElevate,
  now = Date.now(),
}: Props) {
  const disabled = !ready || opening !== null || adminBusy;
  const folders = saved.filter(s => !drives.some(d => isDriveRoot(s.path, d.mount)));
  return (
    <section className="home-content">
      <div className="home-intro">
        <h1>See where your space goes.</h1>
        <p>Map a drive or folder. Follow the big pieces. Everything stays exactly where it is.</p>
        <button
          className="primary"
          disabled={disabled}
          onClick={() => void pickFolder()}
        >
          ▱ &nbsp; Scan a folder…
        </button>
      </div>
      <section className="drives-section">
        <div className="section-heading">
          <h2>Your drives</h2>
          <span>{drives.length} available</span>
        </div>
        {admin?.supported && (
          <div className="admin-row">
            <span>{admin.elevated
              ? 'Running as administrator · drive scans use the fast path.'
              : 'Drive scans are much faster when DiskMapper runs as administrator.'}</span>
            {!admin.elevated && (
              <button disabled={disabled} onClick={() => void restartAsAdmin()}>Restart as administrator</button>
            )}
            <label>
              <input
                type="checkbox"
                checked={admin.alwaysElevate}
                disabled={disabled}
                onChange={e => void setAlwaysElevate(e.target.checked)}
              />
              Always run as administrator
            </label>
          </div>
        )}
        <div className="drive-grid">
          {drives.map(d => {
            const scan = saved.find(s => isDriveRoot(s.path, d.mount));
            const stale = scan && staleness(scan, d, now);
            return (
              <article
                className="drive-card"
                key={d.mount}
              >
                <div className="drive-title">
                  <span className="drive-letter">{d.mount.replace(/[\\/]/g, '')}</span>
                  <div>
                    <h3 title={d.label || 'Local disk'}>{d.label || 'Local disk'}</h3>
                    <span>{d.mount}</span>
                  </div>
                </div>
                <div className="drive-meter">
                  <i style={{ width: `${d.total ? (d.total - d.free) / d.total * 100 : 0}%` }} />
                </div>
                <p><strong>{formatSize(d.total - d.free)}</strong> used of {formatSize(d.total)}</p>
                {scan && (
                  <div className={`last-scan ${stale?.old || stale?.freeDelta != null ? 'warning' : ''}`}>
                    Last scanned {formatAge(scan.scannedAt, now)} · {formatSize(scan.size)}
                  </div>
                )}
                <div className="drive-bottom">
                  <span>{formatSize(d.free)} free</span>
                  <div className="drive-actions">
                    {scan && (
                      <button disabled={disabled} onClick={() => void openSaved(scan.path, scan.scannedAt)}>{opening === scan.path ? 'Opening…' : 'Open last scan'}</button>
                    )}
                    <button
                      disabled={disabled}
                      onClick={() => void start(d.mount)}
                    >
                      Scan drive
                    </button>
                  </div>
                </div>
              </article>
            );
          })}
        </div>
        {!drives.length && (
          <div className="empty-state">
            <h3>No drives found</h3>
            <p>Choose a folder to explore its disk usage.</p>
          </div>
        )}
      </section>
      {folders.length > 0 && (
        <section className="recent-folders">
          <div className="section-heading"><h2>Recent folders</h2></div>
          {folders.map(s => (
            <div className="saved-folder" key={s.path}>
              <div>
                <strong title={s.path}>{middlePath(s.path)}</strong>
                <span className={staleness(s, undefined, now).old ? 'warning' : ''}>{formatAge(s.scannedAt, now)} · {formatSize(s.size)}</span>
              </div>
              <button disabled={disabled} onClick={() => void openSaved(s.path, s.scannedAt)}>{opening === s.path ? 'Opening…' : 'Open'}</button>
            </div>
          ))}
        </section>
      )}
      {saved.length > 0 && (
        <button className="clear-saved" disabled={disabled} onClick={() => void clearSaved()}>Clear saved scans</button>
      )}
      <footer className="home-footer">
        <span>Read-only by design</span>
        <span>Logical file sizes · Links and junctions excluded</span>
      </footer>
    </section>
  );
}
