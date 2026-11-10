import { t, intlLocale } from './i18n';
import { count, formatSize, middlePath, type Progress } from './model';

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
      <h1>{t('scan.title')}</h1>
      <p>{t('scan.description')}</p>
      <div className="scan-counters">
        <div>
          <strong>{progress.files.toLocaleString(intlLocale)}</strong>
          <span>{t('scan.filesFound', { count: progress.files })}</span>
        </div>
        <div>
          <strong>{formatSize(progress.bytes)}</strong>
          <span>{t('scan.sizeFound')}</span>
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
        {count(progress.folders, 'folder')}
        {progress.errors > 0 && ` · ${count(progress.errors, 'unreadable item')}`}
      </div>
      <button
        disabled={cancelling}
        onClick={() => void cancel()}
      >
        {cancelling ? t('scan.cancelling') : t('scan.cancel')}
      </button>
    </section>
  );
}
