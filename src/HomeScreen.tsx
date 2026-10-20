import { formatSize, type Drive } from './model';

interface Props {
  drives: Drive[];
  ready: boolean;
  pickFolder: () => Promise<void>;
  start: (path: string) => Promise<void>;
}

export default function HomeScreen({
  drives,
  ready,
  pickFolder,
  start,
}: Props) {
  return (
    <section className="home-content">
      <div className="home-intro">
        <h1>See where your space goes.</h1>
        <p>Map a drive or folder. Follow the big pieces. Everything stays exactly where it is.</p>
        <button
          className="primary"
          disabled={!ready}
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
        <div className="drive-grid">
          {drives.map(d => (
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
              <div className="drive-bottom">
                <span>{formatSize(d.free)} free</span>
                <button
                  disabled={!ready}
                  onClick={() => void start(d.mount)}
                >
                  Scan drive
                </button>
              </div>
            </article>
          ))}
        </div>
        {!drives.length && (
          <div className="empty-state">
            <h3>No drives found</h3>
            <p>Choose a folder to explore its disk usage.</p>
          </div>
        )}
      </section>
      <footer className="home-footer">
        <span>Read-only by design</span>
        <span>Logical file sizes · Links and junctions excluded</span>
      </footer>
    </section>
  );
}
