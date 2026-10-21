import { renderToStaticMarkup } from 'react-dom/server';
import { Children, isValidElement, type ComponentProps, type ReactElement, type ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import HomeScreen from './HomeScreen';
import ResultsScreen, { SnapshotBanner } from './ResultsScreen';
import { categories, formatSize, percent, type SavedScan, type TreeNode } from './model';

const current: TreeNode = { id: 0, name: 'Other', kind: 'dir', size: 10000, files: 2, folders: 1, category: 'apps' };
const longName = `${'long-name-'.repeat(15)}.pbo`;
const longPath = `${'deeply-nested-folder\\'.repeat(30)}${longName}`;
const noop = () => {};
afterEach(() => vi.unstubAllGlobals());

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
        setAlwaysElevate={async () => {}}
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
    const screen = HomeScreen({ drives: [drive], ready: true, start: async () => {}, pickFolder: async () => {}, saved: [scan, folder], openSaved, clearSaved, restartAsAdmin: async () => {}, setAlwaysElevate: async () => {}, now });
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
    const props = { drives: [drive], ready: true, start: async () => {}, pickFolder: async () => {}, saved: [scan, folder], openSaved: async () => {}, clearSaved: async () => {}, restartAsAdmin: async () => {}, setAlwaysElevate: async () => {}, now };
    for (const opening of [scan.path, folder.path]) {
      const html = renderToStaticMarkup(<HomeScreen {...props} opening={opening} />);
      const controls = html.match(/<button\b[^>]*>[^<]*<\/button>/g)!;
      expect(controls.filter(b => b.includes('Opening…'))).toHaveLength(1);
      expect(controls.every(b => b.includes('disabled=""'))).toBe(true);
    }
    expect(renderToStaticMarkup(<HomeScreen {...props} />)).not.toContain('disabled=""');
  });
  it('styles stale drive scans as warnings and fresh scans neutrally', () => {
    const props = { drives: [drive], ready: true, start: async () => {}, pickFolder: async () => {}, openSaved: async () => {}, clearSaved: async () => {}, restartAsAdmin: async () => {}, setAlwaysElevate: async () => {}, now };
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

describe('administrator controls', () => {
  const props = { drives: [], ready: true, saved: [], start: async () => {}, pickFolder: async () => {}, openSaved: async () => {}, clearSaved: async () => {}, restartAsAdmin: async () => {}, setAlwaysElevate: async () => {} };
  const admin = { supported: true, elevated: false, alwaysElevate: false, declined: false };
  type Control = { children?: ReactNode; checked?: boolean; disabled?: boolean; onClick?: () => void; onChange?: (e: { target: { checked: boolean } }) => void };
  function controls(node: ReactNode): ReactElement<Control>[] {
    return Children.toArray(node).flatMap(child => {
      if (!isValidElement<Control>(child)) return [];
      return child.type === 'button' || child.type === 'input' ? [child] : controls(child.props.children);
    });
  }
  it('offers restart and an unchecked preference when not elevated', () => {
    const screen = HomeScreen({ ...props, admin });
    const html = renderToStaticMarkup(screen);
    expect(html).toContain('Drive scans are much faster when DiskMapper runs as administrator.');
    expect(html).toContain('Restart as administrator');
    expect(html).toContain('Always run as administrator');
    const checkbox = controls(screen).find(c => c.type === 'input')!;
    expect(checkbox.props.checked).toBe(false);
    expect(checkbox.props.disabled).toBe(false);
  });
  it('shows the fast path and keeps the preference when elevated', () => {
    const html = renderToStaticMarkup(<HomeScreen {...props} admin={{ ...admin, elevated: true, alwaysElevate: true }} />);
    expect(html).toContain('Running as administrator · drive scans use the fast path.');
    expect(html).not.toContain('Restart as administrator');
    expect(html).toContain('Always run as administrator');
    expect(html).toContain('checked=""');
  });
  it('hides administrator controls when unsupported or not loaded', () => {
    for (const status of [undefined, null, { ...admin, supported: false }]) {
      const html = renderToStaticMarkup(<HomeScreen {...props} admin={status} />);
      expect(html).not.toContain('admin-row');
      expect(html).not.toContain('administrator');
    }
  });
  it('calls restart and toggles the controlled preference in both directions', () => {
    const restartAsAdmin = vi.fn(async () => {});
    const setAlwaysElevate = vi.fn(async (_enabled: boolean) => {});
    for (const enabled of [false, true]) {
      const screen = HomeScreen({ ...props, admin: { ...admin, alwaysElevate: enabled }, restartAsAdmin, setAlwaysElevate });
      const items = controls(screen);
      if (!enabled) items.find(c => c.props.children === 'Restart as administrator')!.props.onClick!();
      const checkbox = items.find(c => c.type === 'input')!;
      expect(checkbox.props.checked).toBe(enabled);
      checkbox.props.onChange!({ target: { checked: !enabled } });
    }
    expect(restartAsAdmin).toHaveBeenCalledOnce();
    expect(setAlwaysElevate.mock.calls).toEqual([[true], [false]]);
  });
  it('disables restart and the preference while home is busy', () => {
    for (const busy of [{ ready: false }, { opening: 'C:\\' }, { adminBusy: true }]) {
      const items = controls(HomeScreen({ ...props, admin, ...busy }));
      expect(items.find(c => c.props.children === 'Restart as administrator')!.props.disabled).toBe(true);
      expect(items.find(c => c.type === 'input')!.props.disabled).toBe(true);
    }
  });
});
