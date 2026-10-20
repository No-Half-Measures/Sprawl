import { formatSize, middlePath, type Progress } from './model';

interface Props {
  progress: Progress;
  cancelling: boolean;
  cancel: () => Promise<void>;
}

export default function ScanScreen({
  progress,
  cancelling,
  cancel,
}: Props) {
  return (
    <section
      className="scan-screen"
      aria-live="polite"
    >
      <span
        className="scan-symbol"
        aria-hidden="true"
      >
        ▱
      </span>
      <h1>Mapping your space</h1>
      <p>Finding the pieces that make up this folder.</p>
      <div className="scan-counters">
        <div>
          <strong>{progress.files.toLocaleString()}</strong>
          <span>files found</span>
        </div>
        <div>
          <strong>{formatSize(progress.bytes)}</strong>
          <span>size found</span>
        </div>
      </div>
      <div className="indeterminate">
        <i />
      </div>
      <p
        className="current-path"
        title={progress.currentPath}
      >
        {middlePath(progress.currentPath, 90)}
      </p>
      <div className="scan-meta">
        {progress.folders.toLocaleString()} folders
        {progress.errors > 0 && ` · ${progress.errors.toLocaleString()} unreadable items`}
      </div>
      <button
        disabled={cancelling}
        onClick={() => void cancel()}
      >
        {cancelling ? 'Cancelling…' : 'Cancel scan'}
      </button>
    </section>
  );
}
