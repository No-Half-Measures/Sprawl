import { renderToStaticMarkup } from 'react-dom/server';
import * as React from 'react';
import { Children, isValidElement, type ComponentProps, type ReactElement, type ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import HomeScreen from './HomeScreen';
import SettingsMenu from './SettingsMenu';
import ResultsScreen, { SnapshotBanner } from './ResultsScreen';
import DeleteDialog from './DeleteDialog';
import ScanScreen from './ScanScreen';
import { categories, formatSize, percent, type SavedScan, type TreeNode } from './model';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import App from './App';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('react', async importOriginal => {
  const actual = await importOriginal<typeof React>();
  return { ...actual, useState: vi.fn(actual.useState), useRef: vi.fn(actual.useRef), useEffect: vi.fn(actual.useEffect) };
});

const current: TreeNode = { id: 0, name: 'Other', kind: 'dir', size: 10000, files: 2, folders: 1, category: 'apps' };
const longName = `${'long-name-'.repeat(15)}.pbo`;
const longPath = `${'deeply-nested-folder\\'.repeat(30)}${longName}`;
const noop = () => {};
afterEach(() => vi.unstubAllGlobals());

describe('delete confirmation', () => {
  it('renders the item, full path, size and folder counts, with initial focus on Cancel', () => {
    const html = renderToStaticMarkup(<DeleteDialog node={current} path="E:\Projects\Other" running={false} cancel={noop} confirm={noop} />);
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain('aria-modal="true"');
    expect(html).toContain('Delete Other?');
    expect(html.match(/<button/g)).toHaveLength(3);
    expect(html).toContain('Other');
    expect(html).toContain('E:\\Projects\\Other');
    expect(html).toContain('9.8 KB · 2 files · 1 folder');
    expect(html).toContain('Moving to the Recycle Bin lets you restore it later. Deleting permanently frees the space now and can&#x27;t be undone.');
    expect(html).toMatch(/<button autofocus="">Cancel<\/button>/);
    expect(html.match(/autofocus/g)).toHaveLength(1);
    expect(html).toContain('class="primary">Move to Recycle Bin');
    expect(html).toContain('class="danger-outline">Delete permanently');
  });
  it('disables all three buttons while moving and omits folder counts for files', () => {
    const html = renderToStaticMarkup(<DeleteDialog node={{ ...current, kind: 'file' }} path="C:\file.txt" running cancel={noop} confirm={noop} />);
    expect(html.match(/disabled=""/g)).toHaveLength(3);
    expect(html).toContain('Moving…');
    expect(html).not.toContain('2 files');
    expect(html).not.toContain('1 folder');
  });
  it('renders the permanent warning and initially focuses Back', () => {
    const html = renderToStaticMarkup(<DeleteDialog node={current} path="E:\Projects\Other" running={false} cancel={noop} confirm={noop} initialStep="permanent" />);
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain('aria-modal="true"');
    expect(html).toContain('Delete permanently?');
    expect(html).toContain('Other (9.8 KB) will be deleted permanently. This can&#x27;t be undone.');
    expect(html.match(/<button/g)).toHaveLength(2);
    expect(html).toMatch(/<button autofocus="">Back<\/button>/);
    expect(html.match(/autofocus/g)).toHaveLength(1);
    expect(html).toContain('class="danger">Delete permanently');
    expect(html).not.toContain('Move to Recycle Bin');
  });
  it('disables both buttons while permanently deleting', () => {
    const html = renderToStaticMarkup(<DeleteDialog node={current} path="E:\Projects\Other" running cancel={noop} confirm={noop} initialStep="permanent" />);
    expect(html.match(/disabled=""/g)).toHaveLength(2);
    expect(html).toContain('Deleting…');
    expect(html).not.toContain('Moving…');
  });
});

describe('scan counts', () => {
  it('uses singular at one and plural at zero and two', () => {
    for (const n of [0, 1, 2]) {
      const html = renderToStaticMarkup(<ScanScreen progress={{ files: n, folders: n, errors: n, bytes: 0, currentPath: '' }} cancelling={false} cancel={async () => {}} />);
      expect(html).toContain(`<strong>${n}</strong><span>${n === 1 ? 'file' : 'files'} found</span>`);
      expect(html).toContain(`${n} ${n === 1 ? 'folder' : 'folders'}`);
      if (n > 0) expect(html).toContain(`${n} unreadable ${n === 1 ? 'item' : 'items'}`);
      else expect(html).not.toContain('unreadable');
    }
  });
});

describe('screen details', () => {
  const gb = 1024 ** 3;
  const props: ComponentProps<typeof ResultsScreen> = {
    current: { ...current, size: 20 * gb }, trail: [current], history: [],
    parentSize: undefined, parentName: undefined, scanPath: 'C:\\', skipped: 0,
    error: '', loading: false, tree: null, hover: null, linkedHover: null, filter: null,
    largest: [], breakdown: [], back: noop, up: noop, navigate: noop, enter: noop,
    setHover: noop, setFilter: noop, context: noop, action: async () => {},
  };
  const changes = {
    since: 123, previousSize: 19 * gb,
    children: [{ id: 1, previousSize: gb }, { id: 2, previousSize: 2 * gb }, { id: 3, previousSize: null }, { id: 4, previousSize: 0 }],
    removed: { items: 2, bytes: gb }, grown: [],
  };
  it('uses singular labels for one file, folder, removed item and skipped item', () => {
    const drive = { mount: 'C:\\', label: 'Disk', total: 100 * gb, free: 60 * gb };
    const html = renderToStaticMarkup(<ResultsScreen {...props} current={{ ...props.current, files: 1 }} drive={drive} skipped={1} changes={{ ...changes, removed: { items: 1, bytes: gb } }} />);
    expect(html).toContain('<span>1 file</span>');
    expect(html).toContain('<span>1 folder</span>');
    expect(html).toContain('<span>1 removed item</span>');
    expect(html).toContain('1 item couldn’t be read');
    expect(html).toContain('Includes 1 item that couldn&#x27;t be read');
  });
  it('keeps recycled bytes out of Not found and shows them only at the drive root', () => {
    const drive = { mount: 'C:\\', label: 'Disk', total: 100 * gb, free: 60 * gb };
    const html = renderToStaticMarkup(<ResultsScreen {...props} drive={drive} recycled={20 * gb} />);
    expect(html).toContain('· 20 GB in Recycle Bin');
    expect(html).not.toContain('Not found by scan');
    expect(renderToStaticMarkup(<ResultsScreen {...props} recycled={20 * gb} />)).not.toContain('in Recycle Bin');
  });
  it('accounts for both delete modes and labels only recycled bytes', () => {
    const drive = { mount: 'C:\\', label: 'Disk', total: 100 * gb, free: 60 * gb };
    const html = renderToStaticMarkup(<ResultsScreen {...props} drive={drive} recycled={5 * gb} deleted={10 * gb} />);
    expect(html).toContain('Not found <strong>5 GB</strong>');
    expect(html).toContain('· 5 GB in Recycle Bin');
    expect(html).not.toContain('15 GB in Recycle Bin');
    const snapshot: SavedScan = { version: 2, path: drive.mount, scannedAt: 123, size: props.current.size, files: 2, folders: 1, recycled: 5 * gb, deleted: 15 * gb, errors: 0, driveTotal: drive.total, driveFree: drive.free };
    const saved = renderToStaticMarkup(<ResultsScreen {...props} drive={drive} snapshot={snapshot} />);
    expect(saved).not.toContain('Not found');
    expect(saved).toContain('· 5 GB in Recycle Bin');
    const permanentOnly = renderToStaticMarkup(<ResultsScreen {...props} drive={drive} deleted={20 * gb} />);
    expect(permanentOnly).not.toContain('Not found');
    expect(permanentOnly).not.toContain('in Recycle Bin');
  });
  it('renders growth, shrink and new badges, skips aggregates and tiny deltas, and shows removals', () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 });
    const tree: TreeNode = { ...props.current, children: [
      { ...current, id: 1, name: 'grown', size: 2 * gb },
      { ...current, id: 2, name: 'shrunk', size: gb },
      { ...current, id: 3, name: 'new', size: 0 },
      { ...current, id: 4, name: 'tiny', size: 1024 },
      { ...current, id: null, name: '4 smaller items', size: gb },
    ] };
    const html = renderToStaticMarkup(<ResultsScreen {...props} tree={tree} changes={changes} />);
    expect(html).toContain('+1 GB since');
    expect(html).toContain('class="delta growth">+1 GB');
    expect(html).toContain('class="delta shrink">−1 GB');
    expect(html.match(/class="delta growth">New/g)).toHaveLength(1);
    expect(html.match(/class="delta /g)).toHaveLength(4);
    expect(html).toContain('2 removed items');
    expect(html).toContain('Grown since');
    const firstScan = renderToStaticMarkup(<ResultsScreen {...props} tree={tree} />);
    expect(firstScan).not.toContain('class="delta ');
    expect(firstScan).not.toContain('removed items');
    expect(firstScan).not.toContain('Grown since');
  });
  it('labels new and unchanged folders', () => {
    const unchanged = renderToStaticMarkup(<ResultsScreen {...props} changes={{ ...changes, previousSize: props.current.size }} />);
    expect(unchanged).toContain('No change since');
    const added = renderToStaticMarkup(<ResultsScreen {...props} changes={{ ...changes, previousSize: null }} />);
    expect(added).toContain('New since');
  });
  it('uses saved drive capacity for the not-found row and explains unreadable items and elevation', () => {
    const drive = { mount: 'C:\\', label: 'Disk', total: 100 * gb, free: 80 * gb };
    const snapshot: SavedScan = { version: 2, path: drive.mount, scannedAt: 123, size: props.current.size, files: 2, folders: 1, errors: 3, driveTotal: drive.total, driveFree: 70 * gb };
    const html = renderToStaticMarkup(<ResultsScreen {...props} drive={drive} snapshot={snapshot} skipped={3} elevated={false} />);
    expect(html).toContain('Not found <strong>10 GB</strong>');
    expect(html).toContain('Not found by scan');
    expect(html).toContain('Includes 3 items that couldn&#x27;t be read, plus system files and file-system overhead. Run as administrator to include more.');
    expect(html).toContain('width:33.33333333333333%');
    expect(html).toContain('Used <strong>30 GB</strong>');
    const noGap = renderToStaticMarkup(<ResultsScreen {...props} drive={drive} elevated />);
    expect(noGap).not.toContain('Not found');
    const noSkipped = renderToStaticMarkup(<ResultsScreen {...props} drive={drive} snapshot={snapshot} elevated />);
    expect(noSkipped).toContain('System files, restore points, and file-system overhead the scan can&#x27;t see.');
    expect(noSkipped).not.toContain('Run as administrator to include more.');
    expect(renderToStaticMarkup(<ResultsScreen {...props} snapshot={snapshot} />)).not.toContain('Not found');
  });
  it('renders named comparisons, positive categories, tiny percentages, and full path titles', () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 });
    const nestedName = 'long-nested-folder-name-'.repeat(8);
    const html = renderToStaticMarkup(
      <ResultsScreen
        current={current}
        trail={[current]}
        history={[]}
        parentSize={100000}
        parentName="E:\\"
        scanPath="E:\\Projects\\Other"
        skipped={0}
        error=""
        loading={false}
        tree={{ ...current, children: [
          { id: 1, name: longName, kind: 'file', size: 9999, files: 1, folders: 0, category: 'apps' },
          { id: 2, name: nestedName, kind: 'dir', size: 1, files: 1, folders: 0, category: 'code' },
        ] }}
        hover={null}
        linkedHover={null}
        filter={null}
        largest={[{ id: 1, name: longName, relativePath: longPath, size: 9999, category: 'apps' }]}
        breakdown={categories.map(c => ({ category: c.id, bytes: c.id === 'apps' ? 9999 : c.id === 'code' ? 1 : 0, files: c.id === 'apps' || c.id === 'code' ? 1 : 0 }))}
        back={noop}
        up={noop}
        navigate={noop}
        enter={noop}
        setHover={noop}
        setFilter={noop}
        context={noop}
        action={async () => {}}
      />,
    );
    expect(html).toContain('10% of E:');
    expect(html).not.toContain('Scanned');
    expect(html).toContain('Apps &amp; games');
    expect(html).toContain('Code &amp; build');
    expect(html).toContain('title="1 file">1 file</span>');
    expect(html).toContain('title="1 B · 1 file"');
    expect(html).toContain('1 B · &lt;0.1%');
    expect(html).not.toContain('>Video');
    expect(html).not.toContain('>Images');
    expect(html).toContain(`title="${longPath}"`);
    expect(html).toContain(`title="${longName}"`);
    expect(html).toContain(`title="${nestedName}"`);
    expect(html).toContain('class="row-percent">&lt;0.1%');
    expect(html).toContain(`<span class="large-size"><strong>${formatSize(9999)}</strong>`);
    expect(html).toContain(`<small>${percent(9999, 10000)}</small></span>`);
    expect(html).toContain('aria-label="Up one folder (Alt+↑)"');
    expect(html).toContain('role="separator" aria-orientation="horizontal" aria-label="Resize contents and largest files" aria-valuemin="20" aria-valuemax="80" aria-valuenow="48"');
    expect(html).toContain('<aside class="details-panel" style="--contents:48fr;--largest:52fr">');
  });

  it('renders used, free and total capacity for a whole-drive scan', () => {
    const drive = { mount: 'C:\\', label: 'Local disk', total: 1024 ** 4, free: 100 * 1024 ** 3 };
    const html = renderToStaticMarkup(
      <ResultsScreen
        current={current}
        trail={[current]}
        history={[]}
        parentSize={drive.total}
        parentName={drive.mount}
        drive={drive}
        scanPath="C:\\"
        skipped={0}
        error=""
        loading={false}
        tree={null}
        hover={null}
        linkedHover={null}
        filter={null}
        largest={[]}
        breakdown={[]}
        back={noop}
        up={noop}
        navigate={noop}
        enter={noop}
        setHover={noop}
        setFilter={noop}
        context={noop}
        action={async () => {}}
      />,
    );
    expect(html).toContain(`Used <strong>${formatSize(drive.total - drive.free)}</strong>`);
    expect(html).toContain(`Free <strong>${formatSize(drive.free)}</strong>`);
    expect(html).toContain(`Total <strong>${formatSize(drive.total)}</strong>`);
    expect(html).toContain('<span class="drive-capacity"><span>C:\\</span>');
    expect(html).toContain(`Scanned <strong>${formatSize(current.size)}</strong>`);
    expect(html).not.toContain('% of C:');
  });

  it('keeps five drive cards and a single-line home heading', () => {
    const html = renderToStaticMarkup(
      <HomeScreen
        drives={['C','D','E','G','H'].map(letter => ({ mount: `${letter}:\\`, label: 'Local disk', total: 1024, free: 512 }))}
        ready
        pickFolder={async () => {}}
        start={async () => {}}
        saved={[]}
        openSaved={async () => {}}
        clearSaved={async () => {}}
        restartAsAdmin={async () => {}}
      />,
    );
    expect(html.match(/class="drive-card"/g)).toHaveLength(5);
    expect(html).toContain('<h1>See where your space goes.</h1>');
  });
});

describe('saved scan screens', () => {
  const now = new Date(2026, 9, 20, 14, 2).getTime();
  const drive = { mount: 'C:\\', label: 'Disk', total: 1024 ** 4, free: 100 * 1024 ** 3 };
  const scan: SavedScan = { version: 1, path: 'c:/', scannedAt: now - 3 * 86400000, size: 412 * 1024 ** 3, files: 1, folders: 0, errors: 0, driveFree: drive.free, driveTotal: drive.total };
  function buttons(node: ReactNode): ReactElement<{ children?: ReactNode; onClick: () => void }>[] {
    return Children.toArray(node).flatMap(child => {
      if (!isValidElement<{ children?: ReactNode; onClick: () => void }>(child)) return [];
      return child.type === 'button' ? [child] : buttons(child.props.children);
    });
  }
  it('matches drive roots, renders recent folders, and opens the selected saved paths', () => {
    const openSaved = vi.fn(async (_path: string) => {});
    const clearSaved = vi.fn(async () => {});
    const folder = { ...scan, path: 'C:\\Users\\Example' };
    const screen = HomeScreen({ drives: [drive], ready: true, start: async () => {}, pickFolder: async () => {}, saved: [scan, folder], openSaved, clearSaved, restartAsAdmin: async () => {}, now });
    const html = renderToStaticMarkup(screen);
    expect(html).toContain('Last scanned 3 days ago · 412 GB');
    expect(html).toContain('Open last scan');
    expect(html).toContain('<h2>Recent folders</h2>');
    expect(html).toContain(folder.path);
    expect(html.match(/class="saved-folder"/g)).toHaveLength(1);
    for (const text of ['Open last scan', 'Open', 'Clear saved scans']) {
      buttons(screen).find(b => b.props.children === text)!.props.onClick();
    }
    expect(openSaved.mock.calls).toEqual([[scan.path, scan.scannedAt], [folder.path, folder.scannedAt]]);
    expect(clearSaved).toHaveBeenCalledOnce();
  });
  it('labels only the opening snapshot and disables all open and scan buttons', () => {
    const folder = { ...scan, path: 'C:\\Users\\Example' };
    const props = { drives: [drive], ready: true, start: async () => {}, pickFolder: async () => {}, saved: [scan, folder], openSaved: async () => {}, clearSaved: async () => {}, restartAsAdmin: async () => {}, now };
    for (const opening of [scan.path, folder.path]) {
      const html = renderToStaticMarkup(<HomeScreen {...props} opening={opening} />);
      const controls = html.match(/<button\b[^>]*>[^<]*<\/button>/g)!;
      expect(controls.filter(b => b.includes('Opening…'))).toHaveLength(1);
      expect(controls.every(b => b.includes('disabled=""'))).toBe(true);
    }
    expect(renderToStaticMarkup(<HomeScreen {...props} />)).not.toContain('disabled=""');
  });
  it('styles stale drive scans as warnings and fresh scans neutrally', () => {
    const props = { drives: [drive], ready: true, start: async () => {}, pickFolder: async () => {}, openSaved: async () => {}, clearSaved: async () => {}, restartAsAdmin: async () => {}, now };
    expect(renderToStaticMarkup(<HomeScreen {...props} saved={[scan]} />)).not.toContain('last-scan warning');
    expect(renderToStaticMarkup(<HomeScreen {...props} saved={[{ ...scan, scannedAt: now - 8 * 86400000 }]} />)).toContain('last-scan warning');
    expect(renderToStaticMarkup(<HomeScreen {...props} saved={[{ ...scan, driveFree: drive.free + 18 * 1024 ** 3 }]} />)).toContain('last-scan warning');
    const empty = renderToStaticMarkup(<HomeScreen {...props} saved={[]} />);
    expect(empty).not.toContain('Clear saved scans');
    expect(empty).not.toContain('Recent folders');
  });
  it('renders a neutral snapshot with a working Rescan button', () => {
    const rescan = vi.fn();
    const banner = SnapshotBanner({ saved: scan, drive, rescan, now });
    const html = renderToStaticMarkup(banner);
    expect(html).toContain('role="status"');
    expect(html).toContain('Snapshot from ');
    expect(html).toContain(' · 3 days ago');
    expect(html).not.toContain('warning');
    expect(html).not.toContain('This may be out of date.');
    expect(html).not.toContain('Free space has changed');
    buttons(banner)[0].props.onClick();
    expect(rescan).toHaveBeenCalledOnce();
  });
  it('shows both age and space warnings, including negative changes', () => {
    for (const delta of [-18, 18]) {
      const html = renderToStaticMarkup(<SnapshotBanner saved={{ ...scan, scannedAt: now - 8 * 86400000 }} drive={{ ...drive, free: drive.free + delta * 1024 ** 3 }} rescan={noop} now={now} />);
      expect(html).toContain('snapshot warning');
      expect(html).toContain(' · This may be out of date.');
      expect(html).toContain('Free space has changed by 18 GB since this scan.');
    }
  });
});

type Control = { children?: ReactNode; checked?: boolean; disabled?: boolean; onClick?: () => void; onChange?: (e: { target: { checked: boolean } }) => void };
function controls(node: ReactNode): ReactElement<Control>[] {
  return Children.toArray(node).flatMap(child => {
    if (!isValidElement<Control>(child)) return [];
    return child.type === 'button' || child.type === 'input' ? [child] : controls(child.props.children);
  });
}

describe('administrator controls', () => {
  const props = { drives: [], ready: true, saved: [], start: async () => {}, pickFolder: async () => {}, openSaved: async () => {}, clearSaved: async () => {}, restartAsAdmin: async () => {} };
  const admin = { supported: true, elevated: false, alwaysElevate: false, declined: false, contextMenuEnabled: false };
  it('offers restart when not elevated and leaves preferences out of Home', () => {
    const screen = HomeScreen({ ...props, admin });
    const html = renderToStaticMarkup(screen);
    expect(html).toContain('Drive scans are much faster when Sprawl runs as administrator.');
    expect(html).toContain('Restart as administrator');
    expect(html).not.toContain('Always run as administrator');
    expect(html).not.toContain('Explorer');
    expect(html).not.toContain('type="checkbox"');
    expect(controls(screen).find(c => c.props.children === 'Restart as administrator')!.props.disabled).toBe(false);
  });
  it('shows the fast path when elevated', () => {
    const html = renderToStaticMarkup(<HomeScreen {...props} admin={{ ...admin, elevated: true, alwaysElevate: true }} />);
    expect(html).toContain('Running as administrator · drive scans use the fast path.');
    expect(html).not.toContain('Restart as administrator');
    expect(html).not.toContain('type="checkbox"');
  });
  it('hides administrator controls when unsupported or not loaded', () => {
    for (const status of [undefined, null, { ...admin, supported: false }]) {
      const html = renderToStaticMarkup(<HomeScreen {...props} admin={status} />);
      expect(html).not.toContain('admin-row');
      expect(html).not.toContain('administrator');
    }
  });
  it('calls restart', () => {
    const restartAsAdmin = vi.fn(async () => {});
    const screen = HomeScreen({ ...props, admin, restartAsAdmin });
    controls(screen).find(c => c.props.children === 'Restart as administrator')!.props.onClick!();
    expect(restartAsAdmin).toHaveBeenCalledOnce();
  });
  it('disables restart while home is busy', () => {
    for (const busy of [{ ready: false }, { opening: 'C:\\' }, { adminBusy: true }]) {
      const items = controls(HomeScreen({ ...props, admin, ...busy }));
      expect(items.find(c => c.props.children === 'Restart as administrator')!.props.disabled).toBe(true);
    }
  });
});

describe('settings menu', () => {
  const admin = { supported: true, elevated: false, alwaysElevate: false, declined: false, contextMenuEnabled: false };
  const props: ComponentProps<typeof SettingsMenu> = { admin, busy: false, setAlwaysElevate: async () => {}, setContextMenu: async () => {} };

  function mountSettings(initial = props) {
    let open = false;
    let cleanup: (() => void) | undefined;
    let effect: React.EffectCallback;
    let screen: ReactNode;
    const inside = {};
    const focus = vi.fn();
    const listeners = new Map<string, (event: unknown) => void>();
    vi.stubGlobal('document', {
      addEventListener: (type: string, listener: (event: unknown) => void) => listeners.set(type, listener),
      removeEventListener: (type: string, listener: (event: unknown) => void) => {
        if (listeners.get(type) === listener) listeners.delete(type);
      },
    });
    vi.mocked(React.useState).mockImplementation((() => [open, (next: boolean) => { open = next; }]) as typeof React.useState);
    vi.mocked(React.useEffect).mockImplementation(run => { effect = run; });
    function render(next = initial) {
      cleanup?.();
      vi.mocked(React.useRef)
        .mockReturnValueOnce({ current: { contains: (target: unknown) => target === inside } })
        .mockReturnValueOnce({ current: { focus } });
      screen = SettingsMenu(next);
      cleanup = effect() || undefined;
    }
    render();
    return {
      render,
      html: () => renderToStaticMarkup(screen),
      controls: () => controls(screen),
      event: (type: string, event: unknown) => listeners.get(type)?.(event),
      inside, focus, listeners,
      unmount: () => cleanup?.(),
    };
  }
  afterEach(async () => {
    const actual = await vi.importActual<typeof React>('react');
    vi.mocked(React.useState).mockImplementation(actual.useState);
    vi.mocked(React.useRef).mockImplementation(actual.useRef);
    vi.mocked(React.useEffect).mockImplementation(actual.useEffect);
  });

  it('renders the cog closed by default and toggles the panel', () => {
    const menu = mountSettings();
    expect(menu.html()).toContain('aria-label="Settings"');
    expect(menu.html()).toContain('title="Settings"');
    expect(menu.html()).toContain('aria-haspopup="dialog"');
    expect(menu.html()).toContain('aria-expanded="false"');
    expect(menu.html()).toContain('<svg');
    expect(menu.html()).not.toContain('type="checkbox"');
    menu.controls()[0].props.onClick!();
    menu.render();
    expect(menu.html()).toContain('aria-expanded="true"');
    expect(menu.html()).toContain('<h2>Settings</h2>');
    expect(menu.html()).toContain('Always run as administrator');
    expect(menu.html()).toContain('Add &quot;Scan with Sprawl&quot; to Explorer&#x27;s right-click menu');
    expect(menu.controls().filter(c => c.type === 'input')).toHaveLength(2);
    menu.controls()[0].props.onClick!();
    menu.render();
    expect(menu.html()).not.toContain('type="checkbox"');
    expect(menu.listeners.size).toBe(0);
    menu.unmount();
  });
  it('shows the controlled states at either elevation and toggles both preferences in both directions without closing', () => {
    const setAlwaysElevate = vi.fn(async (_enabled: boolean) => {});
    const setContextMenu = vi.fn(async (_enabled: boolean) => {});
    const menu = mountSettings({ ...props, setAlwaysElevate, setContextMenu });
    menu.controls()[0].props.onClick!();
    for (const elevated of [false, true]) {
      for (const enabled of [false, true]) {
        menu.render({ ...props, admin: { ...admin, elevated, alwaysElevate: enabled, contextMenuEnabled: !enabled }, setAlwaysElevate, setContextMenu });
        const checkboxes = menu.controls().filter(c => c.type === 'input');
        expect(checkboxes[0].props.checked).toBe(enabled);
        expect(checkboxes[1].props.checked).toBe(!enabled);
        for (const checkbox of checkboxes) expect(checkbox.props.disabled).toBe(false);
        checkboxes[0].props.onChange!({ target: { checked: !enabled } });
        checkboxes[1].props.onChange!({ target: { checked: enabled } });
        menu.render();
        expect(menu.html()).toContain('aria-expanded="true"');
      }
    }
    expect(setAlwaysElevate.mock.calls).toEqual([[true], [false], [true], [false]]);
    expect(setContextMenu.mock.calls).toEqual([[false], [true], [false], [true]]);
    menu.unmount();
  });
  it('disables both preferences while busy', () => {
    const menu = mountSettings({ ...props, busy: true });
    menu.controls()[0].props.onClick!();
    menu.render();
    const checkboxes = menu.controls().filter(c => c.type === 'input');
    expect(checkboxes).toHaveLength(2);
    expect(checkboxes.every(c => c.props.disabled)).toBe(true);
    menu.unmount();
  });
  it('closes on Escape, returns focus to the cog and removes listeners', () => {
    const menu = mountSettings();
    menu.controls()[0].props.onClick!();
    menu.render();
    menu.event('keydown', { key: 'Enter' });
    menu.render();
    expect(menu.html()).toContain('aria-expanded="true"');
    const preventDefault = vi.fn();
    menu.event('keydown', { key: 'Escape', preventDefault });
    menu.render();
    expect(menu.html()).toContain('aria-expanded="false"');
    expect(menu.html()).not.toContain('type="checkbox"');
    expect(menu.focus).toHaveBeenCalledOnce();
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(menu.listeners.size).toBe(0);
    menu.unmount();
  });
  it('keeps the panel open for inside mousedown and closes it outside', () => {
    const menu = mountSettings();
    menu.controls()[0].props.onClick!();
    menu.render();
    menu.event('mousedown', { target: menu.inside });
    menu.render();
    expect(menu.html()).toContain('aria-expanded="true"');
    menu.event('mousedown', { target: {} });
    menu.render();
    expect(menu.html()).toContain('aria-expanded="false"');
    expect(menu.focus).not.toHaveBeenCalled();
    expect(menu.listeners.size).toBe(0);
    menu.controls()[0].props.onClick!();
    menu.render();
    expect(menu.listeners.size).toBe(2);
    menu.unmount();
    expect(menu.listeners.size).toBe(0);
  });
});

describe('Explorer scan requests', () => {
  const admin = { supported: true, elevated: false, alwaysElevate: false, declined: false, contextMenuEnabled: false };
  const events = new Map<string, (event: { payload: unknown }) => void>();

  function mountApp(path: string | null = null, status = admin) {
    const values: unknown[] = [];
    const effects: { deps?: React.DependencyList; run: React.EffectCallback; pending: boolean; cleanup?: () => void }[] = [];
    let slot = 0;
    let effect = 0;
    let dirty = true;
    let screen: ReactNode;
    vi.mocked(React.useState).mockImplementation((<T,>(initial: T | (() => T)) => {
      const index = slot++;
      if (!(index in values)) values[index] = typeof initial === 'function' ? (initial as () => T)() : initial;
      return [values[index], (next: T | ((value: T) => T)) => {
        values[index] = typeof next === 'function' ? (next as (value: T) => T)(values[index] as T) : next;
        dirty = true;
      }];
    }) as typeof React.useState);
    vi.mocked(React.useRef).mockImplementation((<T,>(initial: T) => {
      const index = slot++;
      if (!(index in values)) values[index] = { current: initial };
      return values[index];
    }) as typeof React.useRef);
    vi.mocked(React.useEffect).mockImplementation((run, deps) => {
      const index = effect++;
      const previous = effects[index];
      if (!previous || !deps || deps.some((value, i) => !Object.is(value, previous.deps?.[i]))) {
        previous?.cleanup?.();
        effects[index] = { run, deps, pending: true };
      }
    });
    vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, setTimeout, clearTimeout });
    vi.stubGlobal('document', { activeElement: null });
    vi.stubGlobal('HTMLElement', class {});
    events.clear();
    vi.mocked(listen).mockImplementation(async (type, handler) => {
      events.set(type as string, handler as (event: { payload: unknown }) => void);
      return () => { events.delete(type as string); };
    });
    let pending = path;
    vi.mocked(invoke).mockReset().mockImplementation(async command => {
      if (command === 'admin_status') return status;
      if (command === 'take_launch_path') {
        const result = pending;
        pending = null;
        return result;
      }
      if (command === 'start_scan') return 1;
      if (command === 'get_node_path') return 'D:\\Photos\\Other';
      return [];
    });
    function render() {
      slot = 0;
      effect = 0;
      dirty = false;
      screen = App();
      for (const item of effects) {
        if (!item.pending) continue;
        item.pending = false;
        item.cleanup = item.run() || undefined;
      }
    }
    render();
    return {
      async settle() {
        for (let i = 0; i < 40; i++) {
          await Promise.resolve();
          if (dirty) render();
        }
      },
      find<T extends React.ElementType>(type: T): ReactElement<ComponentProps<T>> {
        function find(node: ReactNode): ReactElement<ComponentProps<T>> | undefined {
          for (const child of Children.toArray(node)) {
            if (!isValidElement<{ children?: ReactNode }>(child)) continue;
            if (child.type === type) return child as ReactElement<ComponentProps<T>>;
            const found = find(child.props.children);
            if (found) return found;
          }
        }
        const found = find(screen);
        expect(found).toBeDefined();
        return found!;
      },
      html: () => renderToStaticMarkup(screen),
      unmount: () => effects.forEach(item => item.cleanup?.()),
    };
  }
  afterEach(async () => {
    const actual = await vi.importActual<typeof React>('react');
    vi.mocked(React.useState).mockImplementation(actual.useState);
    vi.mocked(React.useRef).mockImplementation(actual.useRef);
    vi.mocked(React.useEffect).mockImplementation(actual.useEffect);
  });

  it('keeps settings at the end of the header on Home, scanning and results', async () => {
    const app = mountApp();
    expect(app.html()).not.toContain('aria-label="Settings"');
    await app.settle();
    for (const phase of ['home', 'scanning', 'results']) {
      app.find(SettingsMenu);
      const actions = Children.toArray(app.find('header').props.children).slice(-1)[0] as ReactElement<{ className: string; children: ReactNode }>;
      expect(actions.props.className).toBe('header-actions');
      expect((Children.toArray(actions.props.children).slice(-1)[0] as ReactElement).type).toBe(SettingsMenu);
      expect(controls(actions).map(c => c.props.children)).toEqual(phase === 'results' ? ['Rescan', 'Home'] : []);
      if (phase === 'home') events.get('scan-requested')!({ payload: 'D:\\Photos' });
      if (phase === 'scanning') events.get('scan-complete')!({ payload: { scanId: 1, root: current } });
      await app.settle();
    }
    app.unmount();
  });

  it('hides settings when administrator controls are unsupported', async () => {
    const app = mountApp(null, { ...admin, supported: false });
    await app.settle();
    expect(app.html()).toContain('class="header-actions"');
    expect(app.html()).not.toContain('aria-label="Settings"');
    app.unmount();
  });

  it('takes the launch path after all listeners are ready and uses the normal scan flow once', async () => {
    const app = mountApp('D:\\Photos');
    expect(invoke).not.toHaveBeenCalledWith('take_launch_path');
    await app.settle();
    expect(events.has('scan-requested')).toBe(true);
    expect(invoke).toHaveBeenCalledWith('start_scan', { path: 'D:\\Photos' });
    expect(app.find(ScanScreen).props.progress.currentPath).toBe('D:\\Photos');
    events.get('scan-cancelled')!({ payload: { scanId: 1 } });
    await app.settle();
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'start_scan')).toHaveLength(1);
    app.find(HomeScreen);
    app.unmount();
    expect(events.size).toBe(0);
  });

  it('starts forwarded paths while home or scanning, and ignores requests during deletion', async () => {
    const app = mountApp();
    await app.settle();
    for (const path of ['D:\\Photos', 'C:\\']) {
      events.get('scan-requested')!({ payload: path });
      await app.settle();
      expect(invoke).toHaveBeenCalledWith('start_scan', { path });
      expect(app.find(ScanScreen).props.progress.currentPath).toBe(path);
    }
    events.get('scan-complete')!({ payload: { scanId: 1, root: current } });
    await app.settle();
    await app.find(ResultsScreen).props.action('delete', { ...current, id: 1 });
    await app.settle();
    app.find(DeleteDialog);
    vi.mocked(invoke).mockClear();
    events.get('scan-requested')!({ payload: 'E:\\' });
    await app.settle();
    expect(invoke).not.toHaveBeenCalledWith('start_scan', { path: 'E:\\' });
    app.unmount();
  });

  it('reloads registry state after toggles and surfaces errors in the error banner', async () => {
    const app = mountApp();
    await app.settle();
    expect(app.find(SettingsMenu).props.admin.contextMenuEnabled).toBe(false);
    vi.mocked(invoke).mockImplementation(async command => {
      if (command === 'set_context_menu') return;
      if (command === 'admin_status') return { ...admin, contextMenuEnabled: true };
      return [];
    });
    await app.find(SettingsMenu).props.setContextMenu(true);
    await app.settle();
    expect(invoke).toHaveBeenCalledWith('set_context_menu', { enabled: true });
    expect(app.find(SettingsMenu).props.admin.contextMenuEnabled).toBe(true);
    vi.mocked(invoke).mockImplementation(async command => {
      if (command === 'set_context_menu') throw 'Registry access denied';
      if (command === 'admin_status') return { ...admin, contextMenuEnabled: true };
      return [];
    });
    await app.find(SettingsMenu).props.setContextMenu(false);
    await app.settle();
    expect(app.find(SettingsMenu).props.admin.contextMenuEnabled).toBe(true);
    expect(app.html()).toContain('Registry access denied');
    app.unmount();
  });
});
