import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import HomeScreen from './HomeScreen';
import ScanScreen from './ScanScreen';
import ResultsScreen, { SnapshotBanner } from './ResultsScreen';
import {
  emptyProgress,
  isDriveRoot,
  type AdminStatus,
  type Breakdown,
  type Changes,
  type Drive,
  type LargeFile,
  type Progress,
  type SavedScan,
  type TreeNode,
} from './model';
import './App.css';

type Phase = 'home' | 'scanning' | 'results';
type ScanMessage = {
  scanId: number;
  root?: TreeNode;
  message?: string;
} & Partial<Progress>;
type Menu = {
  node: TreeNode;
  x: number;
  y: number;
};
const shallow = (node: TreeNode): TreeNode => ({ ...node, children: undefined });
function findTrail(root: TreeNode, id: number): TreeNode[] | null {
  if (root.id === id) return [];
  for (const child of root.children ?? []) {
    if (child.id === id) return [shallow(child)];
    const path = findTrail(child, id);
    if (path) return [shallow(child), ...path];
  }
  return null;
}

export default function App() {
  const [phase, setPhase] = useState<Phase>('home');
  const [drives, setDrives] = useState<Drive[]>([]);
  const [saved, setSaved] = useState<SavedScan[]>([]);
  const [admin, setAdmin] = useState<AdminStatus | null>(null);
  const [adminBusy, setAdminBusy] = useState(false);
  const [snapshot, setSnapshot] = useState<SavedScan | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [scanPath, setScanPath] = useState('');
  const [progress, setProgress] = useState<Progress>(emptyProgress);
  const [skipped, setSkipped] = useState(0);
  const [trail, setTrail] = useState<TreeNode[]>([]);
  const [history, setHistory] = useState<TreeNode[][]>([]);
  const [tree, setTree] = useState<TreeNode | null>(null);
  const [largest, setLargest] = useState<LargeFile[]>([]);
  const [changes, setChanges] = useState<Changes | null>(null);
  const [breakdown, setBreakdown] = useState<Breakdown[]>([]);
  const [hover, setHover] = useState<number | null>(null);
  const [filter, setFilter] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [loading, setLoading] = useState(false);
  const [opening, setOpening] = useState<string | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const scanId = useRef<number | null>(null);
  const queued = useRef<{
    type: string;
    message: ScanMessage;
  }[]>([]);
  const launching = useRef(false);
  const handler = useRef<(type: string, message: ScanMessage) => void>(() => { });
  useEffect(() => {
    let disposed = false;
    invoke<AdminStatus>('admin_status').then(status => {
      if (disposed) return;
      setAdmin(status);
      if (status.declined) setNotice('Continuing without administrator access.');
    }).catch(e => {
      if (!disposed) setError(String(e));
    });
    return () => { disposed = true; };
  }, []);
  async function refreshSaved() {
    try { setSaved(await invoke<SavedScan[]>('list_saved_scans')); }
    catch (e) { setError(String(e)); }
  }
  useEffect(() => {
    if (!notice) return;
    const timeout = window.setTimeout(() => setNotice(''), 3000);
    return () => window.clearTimeout(timeout);
  }, [notice]);
  handler.current = (type, message) => {
    if (launching.current) {
      queued.current.push({ type, message });
      return;
    }
    if (message.scanId !== scanId.current) return;
    if (type === 'scan-progress') setProgress(message as Progress);
    if (type === 'scan-complete') {
      if (!message.root) return;
      setSkipped(message.errors ?? 0);
      setTrail([message.root]);
      setHistory([]);
      setPhase('results');
      setCancelling(false);
    }
    if (type === 'scan-cancelled') {
      setPhase('home');
      setNotice('Scan cancelled. Choose a drive or folder to start again.');
      setCancelling(false);
    }
    if (type === 'scan-error') {
      setError(message.message ?? 'Scan failed. Try another folder.');
      setPhase('home');
      setCancelling(false);
    }
  };
  useEffect(() => {
    let disposed = false;
    const listeners: UnlistenFn[] = [];
    void (async () => {
      try {
        for (const type of ['scan-progress', 'scan-complete', 'scan-cancelled', 'scan-error']) {
          const stop = await listen<ScanMessage>(type, e => handler.current(type, e.payload));
          if (disposed) {
            stop();
            return;
          }
          listeners.push(stop);
        }
        const stop = await listen('saved-scans-changed', () => void refreshSaved());
        if (disposed) { stop(); return; }
        listeners.push(stop);
        if (!disposed) setReady(true);
      } catch (e) {
        if (!disposed) setError(`Desktop connection unavailable: ${String(e)}`);
      }
    })();
    return () => {
      disposed = true;
      listeners.forEach(stop => stop());
    };
  }, []);
  useEffect(() => {
    if (phase !== 'home') return;
    let disposed = false;
    void refreshSaved();
    invoke<Drive[]>('list_drives').then(d => {
      if (!disposed) {
        setDrives(d.sort((a, b) => a.mount.localeCompare(b.mount, 'en-GB', {
          sensitivity: 'base',
          numeric: true,
        })));
      }
    }).catch(e => {
      if (!disposed) setError(String(e));
    });
    return () => {
      disposed = true;
    };
  }, [phase]);
  const currentId = trail[trail.length - 1]?.id;
  useEffect(() => {
    setChanges(null);
    if (phase !== 'results' || currentId == null) return;
    let stale = false;
    invoke<Changes | null>('get_changes', { nodeId: currentId }).then(next => {
      if (!stale) setChanges(next);
    }).catch(e => {
      if (!stale) setError(String(e));
    });
    return () => { stale = true; };
  }, [phase, currentId, trail[0]]);
  useEffect(() => {
    if (phase !== 'results' || currentId == null) return;
    let stale = false;
    setLoading(true);
    setError('');
    setTree(null);
    setLargest([]);
    setBreakdown([]);
    setHover(null);
    setFilter(null);
    setMenu(null);
    void Promise.all([
      invoke<TreeNode>('get_tree', { nodeId: currentId, depth: 3, maxChildren: 100 }),
      invoke<LargeFile[]>('get_largest_files', { nodeId: currentId, limit: 50 }),
      invoke<Breakdown[]>('get_type_breakdown', { nodeId: currentId }),
    ]).then(([next, files, types]) => {
      if (!stale) {
        setTree(next);
        setLargest(files);
        setBreakdown(types);
      }
    })
      .catch(e => {
        if (!stale) setError(String(e));
      }).finally(() => {
        if (!stale) setLoading(false);
      });
    return () => {
      stale = true;
    };
  }, [phase, currentId, trail[0]]);
  async function start(path: string) {
    if (launching.current) return;
    launching.current = true;
    queued.current = [];
    scanId.current = null;
    setSnapshot(null);
    setChanges(null);
    setError('');
    setNotice('');
    setScanPath(path);
    setProgress({ ...emptyProgress, currentPath: path });
    setTree(null);
    setLargest([]);
    setBreakdown([]);
    setPhase('scanning');
    setCancelling(false);
    try {
      scanId.current = await invoke<number>('start_scan', { path });
      launching.current = false;
      const events = queued.current;
      queued.current = [];
      events.forEach(e => handler.current(e.type, e.message));
    } catch (e) {
      launching.current = false;
      setError(String(e));
      setPhase('home');
    }
  }
  async function openSaved(path: string, scannedAt: number) {
    if (launching.current) return;
    launching.current = true;
    setOpening(path);
    scanId.current = null;
    queued.current = [];
    setError('');
    setNotice('');
    try {
      const loaded = await invoke<{ root: TreeNode; errors: number; saved: SavedScan }>('load_saved_scan', { path, scannedAt });
      setScanPath(loaded.saved.path);
      setSnapshot(loaded.saved);
      setSkipped(loaded.errors);
      setTrail([loaded.root]);
      setHistory([]);
      setCancelling(false);
      setPhase('results');
    } catch (e) {
      setError(String(e));
      void refreshSaved();
    } finally {
      setOpening(null);
      queued.current = [];
      launching.current = false;
    }
  }
  async function clearSaved() {
    try { await invoke('clear_saved_scans'); await refreshSaved(); }
    catch (e) { setError(String(e)); }
  }
  async function restartAsAdmin() {
    setAdminBusy(true);
    setError('');
    try { await invoke('restart_as_admin'); }
    catch (e) { setError(String(e)); }
    finally { setAdminBusy(false); }
  }
  async function setAlwaysElevate(enabled: boolean) {
    setAdminBusy(true);
    setError('');
    try {
      await invoke('set_always_elevate', { enabled });
      setAdmin(status => status && { ...status, alwaysElevate: enabled });
    } catch (e) { setError(String(e)); }
    finally { setAdminBusy(false); }
  }
  async function pickFolder() {
    try {
      const path = await open({ directory: true, multiple: false, title: 'Choose a folder to map' });
      if (typeof path === 'string') await start(path);
    } catch (e) {
      setError(String(e));
    }
  }
  async function cancel() {
    setCancelling(true);
    try {
      await invoke('cancel_scan');
    } catch (e) {
      setError(String(e));
      setCancelling(false);
    }
  }
  function navigate(next: TreeNode[]) {
    setChanges(null);
    setHistory(h => [...h, trail]);
    setTrail(next);
  }
  function back() {
    if (history.length) {
      setChanges(null);
      setTrail(history[history.length - 1]);
      setHistory(h => h.slice(0, -1));
    }
  }
  function up() {
    if (trail.length > 1) navigate(trail.slice(0, -1));
  }
  function enter(node: TreeNode) {
    if (node.kind !== 'dir' || node.id === null || loading || !tree) return;
    const path = findTrail(tree, node.id);
    if (path?.length) navigate([...trail, ...path]);
  }
  useEffect(() => {
    const keydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setHover(null);
        setFilter(null);
        setMenu(null);
        return;
      }
      if (
        phase !== 'results' ||
        (e.target instanceof HTMLElement && ['INPUT', 'TEXTAREA'].includes(e.target.tagName))
      ) {
        return;
      }
      if (e.key === 'Backspace' || (e.altKey && e.key === 'ArrowLeft')) {
        e.preventDefault();
        back();
      }
      if (e.altKey && e.key === 'ArrowUp') {
        e.preventDefault();
        up();
      }
    };
    window.addEventListener('keydown', keydown);
    return () => window.removeEventListener('keydown', keydown);
  });
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener('click', close);
    window.addEventListener('resize', close);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('resize', close);
    };
  }, [menu]);
  function context(node: TreeNode, x: number, y: number) {
    if (node.id !== null) {
      setMenu({
        node,
        x: Math.min(x, window.innerWidth - 215),
        y: Math.min(y, window.innerHeight - 155),
      });
    }
  }
  async function action(command: string, node: TreeNode) {
    setMenu(null);
    if (node.id === null) return;
    try {
      if (command === 'copy') {
        const path = await invoke<string>('get_node_path', { nodeId: node.id });
        await navigator.clipboard.writeText(path);
        setNotice('Path copied.');
      } else {
        await invoke(command, { nodeId: node.id });
      }
    } catch (e) {
      setError(String(e));
    }
  }
  const current = trail[trail.length - 1];
  const linkedHover = tree && hover !== null ? findTrail(tree, hover)?.[0]?.id ?? hover : null;
  const drive = drives.filter(d => isDriveRoot(scanPath, d.mount) || scanPath.toLowerCase().startsWith(d.mount.toLowerCase()))
    .sort((a, b) => b.mount.length - a.mount.length)[0];
  const parentSize = trail.length > 1
    ? trail[trail.length - 2].size
    : drive?.total;
  const parentName = trail.length > 1 ? trail[trail.length - 2].name : drive?.mount;
  return (
    <main className={`app ${phase}`}>
      <header className="app-header">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          DiskMapper
        </div>
        <span className="header-note">A little perspective on your space.</span>
        {phase === 'results' && (
          <div className="header-actions">
            <button onClick={() => void start(scanPath)}>Rescan</button>
            <button
              onClick={() => {
                setPhase('home');
                setError('');
              }}
            >
              Home
            </button>
          </div>
        )}
      </header>
      {error && (
        <div
          role="alert"
          className="banner error"
        >
          <span>{error}</span>
          <button
            aria-label="Dismiss error"
            onClick={() => setError('')}
          >
            ×
          </button>
        </div>
      )}
      {notice && (
        <div
          role="status"
          className="toast"
        >
          <span>{notice}</span>
          <button
            aria-label="Dismiss notification"
            onClick={() => setNotice('')}
          >
            ×
          </button>
        </div>
      )}
      {phase === 'home' && (
        <HomeScreen
          drives={drives}
          ready={ready}
          opening={opening}
          pickFolder={pickFolder}
          start={start}
          saved={saved}
          openSaved={openSaved}
          clearSaved={clearSaved}
          admin={admin}
          adminBusy={adminBusy}
          restartAsAdmin={restartAsAdmin}
          setAlwaysElevate={setAlwaysElevate}
        />
      )}
      {phase === 'scanning' && (
        <ScanScreen
          progress={progress}
          cancelling={cancelling}
          cancel={cancel}
        />
      )}
      {phase === 'results' && snapshot && (
        <SnapshotBanner saved={snapshot} drive={drive} rescan={() => void start(scanPath)} />
      )}
      {phase === 'results' && current && (
        <ResultsScreen
          current={current}
          trail={trail}
          history={history}
          parentSize={parentSize}
          parentName={parentName}
          drive={trail.length === 1 && drive && isDriveRoot(scanPath, drive.mount) ? drive : undefined}
          scanPath={scanPath}
          skipped={skipped}
          changes={changes}
          error={error}
          loading={loading}
          tree={tree}
          hover={hover}
          linkedHover={linkedHover}
          filter={filter}
          largest={largest}
          breakdown={breakdown}
          back={back}
          up={up}
          navigate={navigate}
          enter={enter}
          setHover={setHover}
          setFilter={setFilter}
          context={context}
          action={action}
        />
      )}
      {menu && (
        <div
          role="menu"
          aria-label="Item actions"
          className="context-menu"
          style={{ left: menu.x, top: menu.y }}
        >
          <div>{menu.node.name}</div>
          {menu.node.kind === 'dir' && (
            <button
              role="menuitem"
              autoFocus
              onClick={() => void action('open_folder', menu.node)}
            >
              Open folder
            </button>
          )}
          <button
            role="menuitem"
            autoFocus={menu.node.kind !== 'dir'}
            onClick={() => void action('reveal_in_explorer', menu.node)}
          >
            Reveal in Explorer
          </button>
          <button
            role="menuitem"
            onClick={() => void action('copy', menu.node)}
          >
            Copy path
          </button>
        </div>
      )}
    </main>
  );
}
