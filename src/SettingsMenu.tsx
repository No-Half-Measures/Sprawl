import { useEffect, useRef, useState } from 'react';
import type { AdminStatus } from './model';

interface Props {
  admin: AdminStatus;
  busy: boolean;
  setAlwaysElevate: (enabled: boolean) => Promise<void>;
  setContextMenu: (enabled: boolean) => Promise<void>;
}

export default function SettingsMenu({ admin, busy, setAlwaysElevate, setContextMenu }: Props) {
  const [open, setOpen] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    function outside(e: MouseEvent) {
      if (!menu.current?.contains(e.target as Node)) setOpen(false);
    }
    function keydown(e: KeyboardEvent) {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      setOpen(false);
      button.current?.focus();
    }
    document.addEventListener('mousedown', outside);
    document.addEventListener('keydown', keydown);
    return () => {
      document.removeEventListener('mousedown', outside);
      document.removeEventListener('keydown', keydown);
    };
  }, [open]);

  return (
    <div className="settings-menu" ref={menu}>
      <button ref={button} className="settings-button" aria-label="Settings" aria-haspopup="dialog" aria-expanded={open} title="Settings" onClick={() => setOpen(!open)}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
          <path d="m9.5 3-.5 2-2 1-2-.5-2 3.5L4.5 11v2L3 15l2 3.5 2-.5 2 1 .5 2h5l.5-2 2-1 2 .5 2-3.5-1.5-2v-2L21 9l-2-3.5-2 .5-2-1-.5-2Z" />
          <circle cx="12" cy="12" r="3" />
        </svg>
      </button>
      {open && (
        <div className="settings-panel" role="dialog" aria-label="Settings">
          <h2>Settings</h2>
          <label>
            <input type="checkbox" checked={admin.alwaysElevate} disabled={busy} onChange={e => void setAlwaysElevate(e.target.checked)} />
            Always run as administrator
          </label>
          <label>
            <input type="checkbox" checked={admin.contextMenuEnabled} disabled={busy} onChange={e => void setContextMenu(e.target.checked)} />
            Add "Scan with DiskMapper" to Explorer's right-click menu
          </label>
        </div>
      )}
    </div>
  );
}
