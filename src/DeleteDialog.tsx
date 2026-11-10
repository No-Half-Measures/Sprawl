import { t } from './i18n';
import { useRef, useState } from 'react';
import { count, formatSize, type TreeNode } from './model';

export default function DeleteDialog({ node, path, running, cancel, confirm, initialStep = 'choose' }: {
  node: TreeNode; path: string; running: boolean; cancel: () => void; confirm: (permanent: boolean) => void;
  initialStep?: 'choose' | 'permanent';
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const [step, setStep] = useState(initialStep);
  const permanent = step === 'permanent';
  return (
    <div className="dialog-backdrop" onKeyDown={e => {
      e.stopPropagation();
      if (e.key === 'Escape') {
        e.preventDefault();
        if (!running) cancel();
      }
      if (e.key === 'Tab') {
        e.preventDefault();
        const buttons = dialog.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)');
        if (buttons?.length) {
          const index = Array.from(buttons).indexOf(document.activeElement as HTMLButtonElement);
          buttons[(index + (e.shiftKey ? -1 : 1) + buttons.length) % buttons.length].focus();
        }
      }
    }}>
      <div key={step} ref={dialog} role="alertdialog" aria-modal="true" aria-labelledby="delete-title" aria-describedby={permanent ? 'delete-note' : 'delete-details delete-note'} className="delete-dialog" tabIndex={-1}>
        <h2 id="delete-title">{permanent ? t('delete.permanentQuestion') : t('delete.question', { name: node.name })}</h2>
        {!permanent && <div id="delete-details">
          <strong>{node.name}</strong>
          <p className="delete-path">{path}</p>
          <p>{formatSize(node.size)}{node.kind === 'dir' && ` · ${count(node.files, 'file')} · ${count(node.folders, 'folder')}`}</p>
        </div>}
        <p id="delete-note">{permanent
          ? t('delete.warning', { name: node.name, size: formatSize(node.size) })
          : t('delete.explanation')}</p>
        <div className="dialog-actions">
          <button autoFocus disabled={running} onClick={permanent ? () => setStep('choose') : cancel}>{permanent ? t('common.back') : t('common.cancel')}</button>
          <button className={permanent ? 'danger' : 'danger-outline'} disabled={running} onClick={() => {
            if (!permanent) { setStep('permanent'); return; }
            dialog.current?.focus();
            confirm(true);
          }}>{permanent && running ? t('delete.deleting') : t('delete.permanent')}</button>
          {!permanent && <button className="primary" disabled={running} onClick={() => {
            dialog.current?.focus();
            confirm(false);
          }}>{running ? t('delete.moving') : t('delete.recycle')}</button>}
        </div>
      </div>
    </div>
  );
}
