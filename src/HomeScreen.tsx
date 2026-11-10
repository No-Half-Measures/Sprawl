import { t, rich } from './i18n';
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
  now = Date.now(),
}: Props) {
  const disabled = !ready || opening !== null || adminBusy;
  const folders = saved.filter(s => !drives.some(d => isDriveRoot(s.path, d.mount)));
  return (
    <section className="home-content">
      <section className="drives-section">
        <div className="section-heading drives-heading">
          <div>
            <h2>{t('home.drives')}</h2>
            <span>{t('home.available', { count: drives.length })}</span>
            {admin?.supported && admin.elevated && (
              <span>{t('home.elevated')}</span>
            )}
          </div>
          <button
            className="primary"
            disabled={disabled}
            onClick={() => void pickFolder()}
          >
            ▱ &nbsp; {t('home.scanFolder')}
          </button>
        </div>
        {admin?.supported && !admin.elevated && (
          <div className="admin-row">
            <span>{t('home.adminHint')}</span>
            <button disabled={disabled} onClick={() => void restartAsAdmin()}>{t('home.restartAdmin')}</button>
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
                    <h3 title={d.label || t('home.localDisk')}>{d.label || t('home.localDisk')}</h3>
                    <span>{d.mount}</span>
                  </div>
                </div>
                <div className="drive-meter">
                  <i style={{ width: `${d.total ? (d.total - d.free) / d.total * 100 : 0}%` }} />
                </div>
                <p>
                  <span>{rich('home.used', { used: <strong key="used">{formatSize(d.total - d.free)}</strong>, total: formatSize(d.total) })}</span>
                  <span>{t('home.freeSpace', { size: formatSize(d.free) })}</span>
                </p>
                {scan && (
                  <div className={`last-scan ${stale?.old || stale?.freeDelta != null ? 'warning' : ''}`}>
                    {t('home.lastScanned', { age: formatAge(scan.scannedAt, now), size: formatSize(scan.size) })}
                  </div>
                )}
                <div className="drive-actions">
                  {scan && (
                    <button disabled={disabled} onClick={() => void openSaved(scan.path, scan.scannedAt)}>{opening === scan.path ? t('common.opening') : t('home.openLast')}</button>
                  )}
                  <button
                    disabled={disabled}
                    onClick={() => void start(d.mount)}
                  >
                    {t('home.scanDrive')}
                  </button>
                </div>
              </article>
            );
          })}
        </div>
        {!drives.length && (
          <div className="empty-state">
            <h3>{t('home.noDrives')}</h3>
            <p>{t('home.chooseFolder')}</p>
          </div>
        )}
      </section>
      {folders.length > 0 && (
        <section className="recent-folders">
          <div className="section-heading"><h2>{t('home.recent')}</h2></div>
          {folders.map(s => (
            <div className="saved-folder" key={s.path}>
              <div>
                <strong title={s.path}>{middlePath(s.path)}</strong>
                <span className={staleness(s, undefined, now).old ? 'warning' : ''}>{formatAge(s.scannedAt, now)} · {formatSize(s.size)}</span>
              </div>
              <button disabled={disabled} onClick={() => void openSaved(s.path, s.scannedAt)}>{opening === s.path ? t('common.opening') : t('common.open')}</button>
            </div>
          ))}
        </section>
      )}
      {saved.length > 0 && (
        <button className="clear-saved" disabled={disabled} onClick={() => void clearSaved()}>{t('home.clearSaved')}</button>
      )}
    </section>
  );
}
