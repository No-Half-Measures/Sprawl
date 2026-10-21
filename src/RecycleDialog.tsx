import { useRef } from 'react';
import { count, formatSize, type TreeNode } from './model';

export default function RecycleDialog({ node, path, moving, cancel, confirm }: {
  node: TreeNode; path: string; moving: boolean; cancel: () => void; confirm: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  return (
    <div className="dialog-backdrop" onKeyDown={e => {
      e.stopPropagation();
      if (e.key === 'Escape') {
        e.preventDefault();
        if (!moving) cancel();
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
      <div ref={dialog} role="alertdialog" aria-modal="true" aria-labelledby="recycle-title" aria-describedby="recycle-details recycle-note" className="recycle-dialog" tabIndex={-1}>
        <h2 id="recycle-title">Move to Recycle Bin?</h2>
        <div id="recycle-details">
          <strong>{node.name}</strong>
          <p className="recycle-path">{path}</p>
          <p>{formatSize(node.size)}{node.kind === 'dir' && ` · ${count(node.files, 'file')} · ${count(node.folders, 'folder')}`}</p>
        </div>
        <p id="recycle-note">You can restore it from the Recycle Bin. The space is freed when you empty the Recycle Bin.</p>
        <div className="dialog-actions">
          <button autoFocus disabled={moving} onClick={cancel}>Cancel</button>
          <button className="danger" disabled={moving} onClick={() => {
            dialog.current?.focus();
            confirm();
          }}>{moving ? 'Moving…' : 'Move to Recycle Bin'}</button>
        </div>
      </div>
    </div>
  );
}
