import { renderToStaticMarkup } from 'react-dom/server';
import * as React from 'react';
import { Children, isValidElement, type ComponentProps, type ReactElement, type ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import HomeScreen from './HomeScreen';
import SettingsMenu from './SettingsMenu';
import ResultsScreen, { SnapshotBanner } from './ResultsScreen';
import DeleteDialog from './DeleteDialog';
import ScanScreen from './ScanScreen';
import Treemap from './Treemap';
import FilesPanel, { ExtensionsPanel } from './FilesPanel';
import { categories, formatSize, percent, type SavedScan, type SearchResults, type TreeNode, type LargeFile, type FilePage } from './model';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import App from './App';
import { setPreference } from './i18n';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('react', async importOriginal => {
  const actual = await importOriginal<typeof React>();
  return { ...actual, useState: vi.fn(actual.useState), useRef: vi.fn(actual.useRef), useEffect: vi.fn(actual.useEffect) };
});

const current: TreeNode = { id: 0, name: 'Other', kind: 'dir', size: 10000, allocated: 10000, files: 2, folders: 1, category: 'apps' };
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
    expect(html).toContain('1 item couldn&#x27;t be read');
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
  it('shows size on disk in the summary, rows and Not found only when it is known', () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 });
    const drive = { mount: 'C:\\', label: 'Disk', total: 100 * gb, free: 60 * gb };
    const known = { ...props, current: { ...props.current, allocated: 38 * gb }, drive, hasAllocated: true,
      tree: { ...props.current, allocated: 38 * gb, children: [{ ...current, id: 1, name: 'data', size: 20 * gb, allocated: 38 * gb }] },
      largest: [{ id: 2, name: 'big.vhdx', relativePath: 'data\\big.vhdx', size: 20 * gb, allocated: 3 * gb, category: 'archives' }] };
    const html = renderToStaticMarkup(<ResultsScreen {...known} />);
    expect(html).toContain('Scanned <strong>20 GB</strong></span><span title="Space the files take up on the drive');
    expect(html).toContain('>· 38 GB on disk</span>');
    expect(html).toContain('<strong class="row-size" title="38 GB on disk">');
    expect(html).toContain('<span class="large-size" title="3 GB on disk">');
    expect(html).toContain('Not found <strong>2 GB</strong>');
    const unknown = renderToStaticMarkup(<ResultsScreen {...known} hasAllocated={false} />);
    expect(unknown).not.toContain('on disk');
    expect(unknown).toContain('<strong class="row-size">');
    expect(unknown).toContain('Not found <strong>20 GB</strong>');
  });
  it('shows size on disk in the treemap tooltip only when it is known', async () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 });
    const actual = await vi.importActual<typeof React>('react');
    const node: TreeNode = { ...current, id: 1, name: 'data', size: 4 * gb, allocated: 3 * gb };
    const tip = { tile: { node, x: 0, y: 0, width: 10, height: 10, depth: 1, ancestors: [], branch: 1, frame: false }, x: 1, y: 1 };
    const render = (showAllocated: boolean) => {
      let calls = 0;
      vi.mocked(React.useState).mockImplementation(((initial: unknown) => calls++ === 2 ? [tip, noop] : actual.useState(initial)) as typeof React.useState);
      return renderToStaticMarkup(<Treemap tree={{ ...current, size: 8 * gb }} showAllocated={showAllocated} hover={null} filter={null} onHover={noop} onOpen={noop} onContext={noop} />);
    };
    try {
      expect(render(true)).toContain('<span>4 GB · 50%</span><small>3 GB on disk</small>');
      expect(render(false)).not.toContain('on disk');
    } finally {
      vi.mocked(React.useState).mockImplementation(actual.useState);
    }
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
          { id: 1, name: longName, kind: 'file', size: 9999, allocated: 12288, files: 1, folders: 0, category: 'apps' },
          { id: 2, name: nestedName, kind: 'dir', size: 1, allocated: 0, files: 1, folders: 0, category: 'code' },
        ] }}
        hover={null}
        linkedHover={null}
        filter={null}
        largest={[{ id: 1, name: longName, relativePath: longPath, size: 9999, allocated: 12288, category: 'apps' }]}
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
    expect(html).not.toContain('on disk');
    expect(html).toContain(`<small>${percent(9999, 10000)}</small></span>`);
    expect(html).toContain('aria-label="Up one folder (Alt+↑)"');
    expect(html).toContain('role="separator" aria-orientation="horizontal" aria-label="Resize contents and largest files" aria-valuemin="20" aria-valuemax="80" aria-valuenow="48"');
    expect(html).toContain('<aside class="details-panel" style="--contents:48fr;--largest:52fr">');
  });

  it('replaces Contents and Largest files with search results while a query is active', () => {
    const search: SearchResults = { total: 1204, results: [
      { id: 7, name: 'win11.iso', relativePath: 'isos\\win11.iso', kind: 'file', size: 6 * gb, allocated: 5 * gb, category: 'archives' },
      { id: 8, name: 'isos', relativePath: 'isos', kind: 'dir', size: 6 * gb, allocated: 5 * gb },
    ] };
    const html = renderToStaticMarkup(<ResultsScreen {...props} hasAllocated query="iso" search={search} />);
    expect(html).toContain('<h2>Search</h2><span>1,204 matches, showing largest 2</span>');
    expect(html).toContain('class="details-panel searching"');
    expect(html).not.toContain('<h2>Contents</h2>');
    expect(html).not.toContain('Largest files');
    expect(html).toContain('title="isos\\win11.iso\nReveal in Explorer"');
    expect(html).toContain('<strong title="win11.iso">win11.iso</strong><small title="isos\\win11.iso">isos\\win11.iso</small>');
    expect(html).toContain('<span class="large-size" title="5 GB on disk"><strong>6 GB</strong><small>30%</small></span>');
    expect(html).toContain('title="isos\nOpen folder"');
    expect(html).toContain('<strong title="isos">▱ isos</strong>');
    expect(html).toContain('value="iso"');
    expect(html).not.toContain('search-error');

    const single = renderToStaticMarkup(<ResultsScreen {...props} query="iso" search={{ total: 1, results: [search.results[0]] }} />);
    expect(single).toContain('<span>1 match</span>');
    expect(single).not.toContain('on disk');
    const empty = renderToStaticMarkup(<ResultsScreen {...props} query="nothing" search={{ total: 0, results: [] }} />);
    expect(empty).toContain('<span>0 matches</span>');
    expect(empty).toContain('<p class="quiet-empty">No matches</p>');
    expect(renderToStaticMarkup(<ResultsScreen {...props} query="iso" />)).toContain('Searching…');
    const idle = renderToStaticMarkup(<ResultsScreen {...props} query="  " search={search} />);
    expect(idle).toContain('<h2>Contents</h2>');
    expect(idle).not.toContain('<h2>Search</h2>');
  });
  it('shows search errors inline and hints at the query syntax', () => {
    const html = renderToStaticMarkup(<ResultsScreen {...props} query="type:movies" searchError={'Unknown type "movies".'} />);
    expect(html).toContain('aria-invalid="true" aria-describedby="search-error"');
    expect(html).toContain('<small id="search-error" class="search-error">Unknown type &quot;movies&quot;.</small>');
    expect(html).not.toContain('Searching…');
    expect(html).not.toContain('No matches');
    expect(html).toContain('placeholder="Search… *.iso size&gt;1GB type:video"');
    expect(html).toMatch(/title="Search everything under this folder[^"]*size&gt;1GB[^"]*type:video[^"]*kind:file[^"]*Ctrl\+F/);
  });
  it('renders the export button and its running state', () => {
    expect(renderToStaticMarkup(<ResultsScreen {...props} />)).toContain('<button title="Save every item in this folder as CSV or JSON">Export…</button>');
    expect(renderToStaticMarkup(<ResultsScreen {...props} exporting />)).toContain('<button disabled="" title="Save every item in this folder as CSV or JSON">Exporting…</button>');
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

  it('keeps five drive cards without home marketing copy', () => {
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
    expect(html).not.toContain('<h1');
    expect(html).not.toContain('Read-only by design');
    expect(html).not.toContain('Logical file sizes');
    expect(html).toContain('Scan a folder');
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
    expect(html).not.toContain('admin-row');
    const heading = html.match(/<div class="section-heading drives-heading">(.*?)<\/button><\/div>/)?.[1];
    expect(heading).toContain('Running as administrator · drive scans use the fast path.');
    expect(heading).toContain('Scan a folder');
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
      if (command === 'get_changes') return null;
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
    expect(app.html()).toContain('aria-label="Settings"');
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

  it('keeps settings when administrator controls are unsupported', async () => {
    const app = mountApp(null, { ...admin, supported: false });
    await app.settle();
    expect(app.html()).toContain('class="header-actions"');
    expect(app.html()).toContain('aria-label="Settings"');
    app.unmount();
  });

  it('switches language from Settings, persists it and updates the backend', async () => {
    const storage = new Map<string, string>();
    vi.stubGlobal('localStorage', { getItem: (key: string) => storage.get(key), setItem: (key: string, value: string) => storage.set(key, value) });
    const app = mountApp(null, { ...admin, supported: false });
    await app.settle();
    const root = { lang: '' };
    Object.assign(document, { documentElement: root });
    expect(invoke).toHaveBeenCalledWith('set_locale', { locale: 'en' });
    const effect = vi.mocked(React.useEffect).getMockImplementation()!;
    const ref = vi.mocked(React.useRef).getMockImplementation()!;
    vi.mocked(React.useEffect).mockImplementation(noop);
    vi.mocked(React.useRef).mockImplementation((initial => ({ current: initial })) as typeof React.useRef);
    vi.mocked(React.useState).mockReturnValueOnce([true, noop]);
    const panel = SettingsMenu(app.find(SettingsMenu).props);
    vi.mocked(React.useEffect).mockImplementation(effect);
    vi.mocked(React.useRef).mockImplementation(ref);
    let select: ReactElement<React.ComponentProps<'select'>> | undefined;
    function walk(node: ReactNode) {
      for (const child of Children.toArray(node)) {
        if (!isValidElement<{ children?: ReactNode }>(child)) continue;
        if (child.type === 'select') select = child as typeof select;
        walk(child.props.children);
      }
    }
    walk(panel);
    expect(renderToStaticMarkup(panel)).not.toContain('type="checkbox"');
    expect(select).toBeDefined();
    try {
      select!.props.onChange!({ target: { value: 'fr' } } as React.ChangeEvent<HTMLSelectElement>);
      await app.settle();
      expect(app.html()).toContain('Vos disques');
      expect(app.html()).toContain('aria-label="Paramètres"');
      expect(storage.get('sprawl.language')).toBe('fr');
      expect(root.lang).toBe('fr');
      expect(invoke).toHaveBeenCalledWith('set_locale', { locale: 'fr' });
      vi.mocked(invoke).mockRejectedValueOnce('Locale unavailable');
      app.find(SettingsMenu).props.setLanguage!('de');
      await app.settle();
      expect(app.html()).toContain('role="alert"');
      expect(app.html()).toContain('Locale unavailable');
    } finally {
      app.unmount();
      setPreference('system');
    }
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

  async function results() {
    const app = mountApp();
    await app.settle();
    events.get('scan-requested')!({ payload: 'D:\\Photos' });
    await app.settle();
    events.get('scan-complete')!({ payload: { scanId: 1, hasModified: true, root: { ...current, name: 'Photos' } } });
    await app.settle();
    expect(app.find(ResultsScreen).props.hasModified).toBe(true);
    const base = vi.mocked(invoke).getMockImplementation()!;
    return { app, base };
  }

  it('passes date availability through saved scans, including legacy scans', async () => {
    for (const hasModified of [true, false]) {
      const app = mountApp();
      await app.settle();
      const base = vi.mocked(invoke).getMockImplementation()!;
      const saved: SavedScan = { version: hasModified ? 5 : 4, path: 'D:', scannedAt: 1, size: current.size, files: current.files, folders: current.folders, errors: 0, driveFree: null, driveTotal: null };
      vi.mocked(invoke).mockImplementation(async (command, args) => command === 'load_saved_scan' ? { root: current, errors: 0, hasAllocated: true, hasModified, saved } : base(command, args));
      await app.find(HomeScreen).props.openSaved('D:', 1);
      await app.settle();
      expect(app.find(ResultsScreen).props.hasModified).toBe(hasModified);
      app.unmount();
    }
  });

  it('debounces searches, scopes them to the current folder and shows results or errors', async () => {
    vi.useFakeTimers();
    try {
      const { app, base } = await results();
      const found: SearchResults = { total: 3, results: [{ id: 4, name: 'a.iso', relativePath: 'a.iso', kind: 'file', size: 9, allocated: 9 }] };
      vi.mocked(invoke).mockImplementation(async (command, args) => {
        if (command === 'search') {
          if ((args as { query: string }).query === 'type:nope') throw 'Unknown type "nope".';
          return found;
        }
        return base(command, args);
      });
      app.find(ResultsScreen).props.setQuery!(' *.iso ');
      await app.settle();
      expect(invoke).not.toHaveBeenCalledWith('search', expect.anything());
      vi.advanceTimersByTime(200);
      await app.settle();
      expect(invoke).toHaveBeenCalledWith('search', { nodeId: 0, query: '*.iso', limit: 200 });
      expect(app.find(ResultsScreen).props.search).toEqual(found);
      expect(app.find(ResultsScreen).props.query).toBe(' *.iso ');

      app.find(ResultsScreen).props.setQuery!('type:nope');
      await app.settle();
      expect(app.find(ResultsScreen).props.search).toBeNull();
      vi.advanceTimersByTime(200);
      await app.settle();
      expect(app.find(ResultsScreen).props.searchError).toBe('Unknown type "nope".');
      expect(app.html()).not.toContain('role="alert"');

      vi.mocked(invoke).mockClear();
      app.find(ResultsScreen).props.setQuery!('');
      await app.settle();
      vi.advanceTimersByTime(200);
      await app.settle();
      expect(invoke).not.toHaveBeenCalledWith('search', expect.anything());
      expect(app.find(ResultsScreen).props.searchError).toBe('');
      app.unmount();
    } finally {
      vi.useRealTimers();
    }
  });

  it('opens folder matches through their trail', async () => {
    const { app, base } = await results();
    const folder = { ...current, id: 5, name: 'Raw' };
    const middle = { ...current, id: 3, name: '2024' };
    vi.mocked(invoke).mockImplementation(async (command, args) => command === 'get_trail' ? [current, middle, folder] : base(command, args));
    app.find(ResultsScreen).props.openMatch!({ id: 5, name: 'Raw', relativePath: '2024\\Raw', kind: 'dir', size: 1, allocated: 1 });
    await app.settle();
    expect(invoke).toHaveBeenCalledWith('get_trail', { nodeId: 5 });
    expect(app.find(ResultsScreen).props.trail.map(n => n.name)).toEqual(['Other', '2024', 'Raw']);
    expect(app.find(ResultsScreen).props.history).toHaveLength(1);
    app.unmount();
  });

  it('exports the current folder in the format of the chosen file', async () => {
    const { app, base } = await results();
    let finish: (items: number) => void = noop;
    vi.mocked(invoke).mockImplementation((command, args) => command === 'export_scan'
      ? new Promise(resolve => { finish = resolve; })
      : base(command, args));
    vi.mocked(save).mockResolvedValueOnce('D:\\Exports\\Photos.JSON');
    app.find(ResultsScreen).props.exportScan!();
    await app.settle();
    expect(save).toHaveBeenCalledWith(expect.objectContaining({
      defaultPath: 'Photos.csv',
      filters: [{ name: 'CSV', extensions: ['csv'] }, { name: 'JSON', extensions: ['json'] }],
    }));
    expect(invoke).toHaveBeenCalledWith('export_scan', { nodeId: 0, path: 'D:\\Exports\\Photos.JSON', format: 'json' });
    expect(app.find(ResultsScreen).props.exporting).toBe(true);
    finish(12345);
    await app.settle();
    expect(app.find(ResultsScreen).props.exporting).toBe(false);
    expect(app.html()).toContain('Exported 12,345 items to Photos.JSON.');

    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'export_scan') throw 'Could not export to D:\\x.csv: denied';
      return base(command, args);
    });
    vi.mocked(save).mockResolvedValueOnce('D:\\x.csv');
    app.find(ResultsScreen).props.exportScan!();
    await app.settle();
    expect(invoke).toHaveBeenCalledWith('export_scan', { nodeId: 0, path: 'D:\\x.csv', format: 'csv' });
    expect(app.html()).toContain('role="alert"');
    expect(app.html()).toContain('Could not export to D:\\x.csv: denied');

    vi.mocked(invoke).mockClear();
    vi.mocked(save).mockResolvedValueOnce(null);
    app.find(ResultsScreen).props.exportScan!();
    await app.settle();
    expect(invoke).not.toHaveBeenCalledWith('export_scan', expect.anything());
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


async function mountPanel<P extends object>(component: (props: P) => ReactNode, initial: P) {
  const actual = await vi.importActual<typeof React>('react');
  const values: unknown[] = [];
  const effects: { deps?: React.DependencyList; run: React.EffectCallback; pending: boolean; cleanup?: () => void }[] = [];
  let slot = 0, effect = 0;
  let dirty = true;
  let props = initial;
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
  vi.stubGlobal('window', { setTimeout, clearTimeout, devicePixelRatio: 1 });
  function render() {
    slot = 0; effect = 0; dirty = false;
    screen = component(props);
    for (const item of effects) {
      if (!item.pending) continue;
      item.pending = false;
      item.cleanup = item.run() || undefined;
    }
  }
  function all(type: React.ElementType): ReactElement<Record<string, any>>[] {
    const result: ReactElement<Record<string, any>>[] = [];
    function walk(node: ReactNode) {
      for (const child of Children.toArray(node)) {
        if (!isValidElement<{ children?: ReactNode }>(child)) continue;
        if (child.type === type) result.push(child as ReactElement<Record<string, any>>);
        walk(child.props.children);
      }
    }
    walk(screen);
    return result;
  }
  render();
  return {
    all,
    find: (type: React.ElementType, predicate: (props: Record<string, any>) => boolean) => {
      const found = all(type).find(element => predicate(element.props));
      expect(found).toBeDefined();
      return found!;
    },
    async settle() { for (let i = 0; i < 40; i++) { await Promise.resolve(); if (dirty) render(); } },
    update(next: Partial<P>) { props = { ...props, ...next }; render(); },
    html() {
      const state = vi.mocked(React.useState).getMockImplementation();
      const ref = vi.mocked(React.useRef).getMockImplementation();
      const effect = vi.mocked(React.useEffect).getMockImplementation();
      vi.mocked(React.useState).mockImplementation(actual.useState);
      vi.mocked(React.useRef).mockImplementation(actual.useRef);
      vi.mocked(React.useEffect).mockImplementation(actual.useEffect);
      try { return renderToStaticMarkup(screen); }
      finally {
        vi.mocked(React.useState).mockImplementation(state!);
        vi.mocked(React.useRef).mockImplementation(ref!);
        vi.mocked(React.useEffect).mockImplementation(effect!);
      }
    },
    unmount() { effects.forEach(item => item.cleanup?.()); },
  };
}

describe('file analysis panels', () => {
  const file = (id: number): LargeFile => ({ id, name: 'file' + id + '.log', relativePath: 'logs/file' + id + '.log', size: 100, allocated: 128, modified: Date.UTC(2024, 2, 12), category: 'other' });
  const renderFile = (f: LargeFile) => <button className="large-row" key={f.id}>{f.name}</button>;
  const props: ComponentProps<typeof ResultsScreen> = { current, trail: [current], history: [], parentSize: undefined, parentName: undefined, scanPath: 'E:', skipped: 0, error: '', loading: false, tree: null, hover: null, linkedHover: null, filter: null, largest: [file(1)], breakdown: [], back: noop, up: noop, navigate: noop, enter: noop, setHover: noop, setFilter: noop, context: noop, action: async () => {} };
  afterEach(async () => {
    const actual = await vi.importActual<typeof React>('react');
    vi.mocked(React.useState).mockImplementation(actual.useState);
    vi.mocked(React.useRef).mockImplementation(actual.useRef);
    vi.mocked(React.useEffect).mockImplementation(actual.useEffect);
    vi.useRealTimers();
  });
  it('always shows the three tabs, adds growth when available, and supports arrow navigation', async () => {
    vi.mocked(invoke).mockReset();
    const panel = await mountPanel(ResultsScreen, { ...props, tree: current });
    expect(panel.all('button').filter(b => b.props.role === 'tab').map(b => b.props.children)).toEqual(['Largest files', 'All files', 'Extensions']);
    expect(invoke).not.toHaveBeenCalled();
    panel.find('button', p => p.children === 'All files').props.onClick();
    await panel.settle();
    expect(panel.all(FilesPanel)).toHaveLength(1);
    expect(panel.all(ExtensionsPanel)).toHaveLength(0);
    panel.find('button', p => p.children === 'Extensions').props.onClick();
    await panel.settle();
    expect(panel.all(FilesPanel)).toHaveLength(0);
    expect(panel.all(ExtensionsPanel)).toHaveLength(1);
    const focus = vi.fn(), preventDefault = vi.fn();
    panel.find('div', p => p.role === 'tablist').props.onKeyDown({ key: 'Home', preventDefault, currentTarget: { querySelectorAll: () => [{ focus }] } });
    await panel.settle();
    expect(focus).toHaveBeenCalled();
    expect(panel.find('button', p => p.children === 'Largest files').props['aria-selected']).toBe(true);
    panel.update({ changes: { since: 1, previousSize: 0, children: [], removed: { items: 0, bytes: 0 }, grown: [] } });
    expect(panel.all('button').filter(b => b.props.role === 'tab')).toHaveLength(4);
    panel.unmount();
  });
  it('fetches 200 files, appends more, and resets when sort or tree changes', async () => {
    vi.useFakeTimers();
    vi.mocked(invoke).mockReset().mockImplementation(async (_command, args) => ({ total: 201, files: (args as { offset: number }).offset ? [file(201)] : Array.from({ length: 200 }, (_, i) => file(i + 1)) }));
    const panel = await mountPanel(FilesPanel, { tree: current, hasModified: true, renderFile });
    vi.advanceTimersByTime(0); await panel.settle();
    expect(invoke).toHaveBeenLastCalledWith('get_files', { nodeId: 0, query: '', sort: 'size', descending: true, offset: 0, limit: 200 });
    expect(panel.html()).toContain('201 files');
    expect(panel.all('button').filter(b => b.props.className === 'large-row')).toHaveLength(200);
    panel.find('button', p => p.children === 'Show more').props.onClick(); await panel.settle();
    expect(invoke).toHaveBeenLastCalledWith('get_files', expect.objectContaining({ offset: 200 }));
    expect(panel.all('button').filter(b => b.props.className === 'large-row')).toHaveLength(201);
    expect(panel.html()).not.toContain('Show more');
    for (const [sort, descending] of [['size', false], ['name', false], ['name', true], ['modified', false], ['modified', true]] as const) {
      panel.find('button', p => p['aria-pressed'] !== undefined && String(p.children[0]).toLowerCase() === sort).props.onClick();
      await panel.settle(); vi.advanceTimersByTime(0); await panel.settle();
      expect(invoke).toHaveBeenLastCalledWith('get_files', expect.objectContaining({ sort, descending, offset: 0 }));
    }
    panel.update({ tree: { ...current, id: 7 } });
    vi.advanceTimersByTime(0); await panel.settle();
    expect(invoke).toHaveBeenLastCalledWith('get_files', expect.objectContaining({ nodeId: 7, offset: 0 }));
    panel.unmount();
  });
  it('debounces filters, shows parse errors, and ignores stale responses including pages', async () => {
    vi.useFakeTimers();
    let finish: (page: FilePage) => void = noop;
    vi.mocked(invoke).mockReset().mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    const panel = await mountPanel(FilesPanel, { tree: current, hasModified: false, renderFile });
    vi.advanceTimersByTime(0); await panel.settle();
    const stale = finish;
    expect(panel.html()).not.toContain('Modified');
    panel.find('input', p => p['aria-label'] === 'Filter all files').props.onChange({ target: { value: 'age>1y' } });
    await panel.settle();
    stale({ total: 1, files: [file(99)] }); await panel.settle();
    expect(panel.html()).not.toContain('file99.log');
    vi.advanceTimersByTime(199); expect(invoke).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1); await panel.settle();
    expect(invoke).toHaveBeenLastCalledWith('get_files', expect.objectContaining({ query: 'age>1y', offset: 0 }));
    finish({ total: 2, files: [file(1)] }); await panel.settle();
    panel.find('button', p => p.children === 'Show more').props.onClick(); await panel.settle();
    const stalePage = finish;
    vi.mocked(invoke).mockRejectedValueOnce('Invalid date. Use YYYY-MM-DD (UTC).');
    panel.find('input', p => p['aria-label'] === 'Filter all files').props.onChange({ target: { value: 'modified>bad' } });
    await panel.settle(); vi.advanceTimersByTime(200); await panel.settle();
    stalePage({ total: 2, files: [file(88)] }); await panel.settle();
    expect(panel.html()).toContain('aria-invalid="true" aria-describedby="files-error"');
    expect(panel.html()).toContain('Invalid date. Use YYYY-MM-DD (UTC).');
    expect(panel.html()).not.toContain('file88.log');
    panel.unmount();
  });
  it('uses the shared file row for all files, with reveal, hover, context and Delete actions', async () => {
    const action = vi.fn(async () => {}), context = vi.fn(), setHover = vi.fn();
    const panel = await mountPanel(ResultsScreen, { ...props, tree: current, hasModified: true, action, context, setHover, linkedHover: 1 });
    panel.find('button', p => p.children === 'All files').props.onClick(); await panel.settle();
    const row = panel.all(FilesPanel)[0].props.renderFile(file(1), 0) as ReactElement<React.ComponentProps<'button'>>;
    expect(row.props.className).toBe('large-row linked-hover');
    row.props.onClick!({} as React.MouseEvent<HTMLButtonElement>);
    expect(action).toHaveBeenCalledWith('reveal_in_explorer', expect.objectContaining({ id: 1, kind: 'file' }));
    row.props.onMouseEnter!({} as React.MouseEvent<HTMLButtonElement>); expect(setHover).toHaveBeenLastCalledWith(1);
    row.props.onMouseLeave!({} as React.MouseEvent<HTMLButtonElement>); expect(setHover).toHaveBeenLastCalledWith(null);
    const preventDefault = vi.fn();
    row.props.onContextMenu!({ preventDefault, clientX: 10, clientY: 20 } as unknown as React.MouseEvent<HTMLButtonElement>);
    expect(context).toHaveBeenCalledWith(expect.objectContaining({ id: 1 }), 10, 20);
    row.props.onKeyDown!({ key: 'Delete', preventDefault } as unknown as React.KeyboardEvent<HTMLButtonElement>);
    expect(action).toHaveBeenLastCalledWith('delete', expect.objectContaining({ id: 1 }));
    panel.unmount();
  });

  it('renders extension totals, meters and allocation tooltips, and selects a search', async () => {
    vi.mocked(invoke).mockReset().mockResolvedValue([{ extension: 'log', category: 'other', bytes: 5000, allocated: 4096, files: 2 }, { extension: '', category: 'other', bytes: 100, allocated: 128, files: 1 }]);
    const setQuery = vi.fn();
    const panel = await mountPanel(ExtensionsPanel, { tree: current, hasAllocated: true, setQuery });
    await panel.settle();
    expect(invoke).toHaveBeenCalledWith('get_extension_breakdown', { nodeId: 0 });
    const html = panel.html();
    expect(html).toContain('.log'); expect(html).toContain('No extension'); expect(html).toContain('2 files');
    expect(html).toContain('50%'); expect(html).toContain('width:50%'); expect(html).toContain('4 KB on disk');
    expect(panel.all('button')).toHaveLength(1);
    panel.all('button')[0].props.onClick(); expect(setQuery).toHaveBeenCalledWith('*.log');
    panel.update({ tree: { ...current, id: 7 }, hasAllocated: false }); await panel.settle();
    expect(invoke).toHaveBeenLastCalledWith('get_extension_breakdown', { nodeId: 7 });
    expect(panel.html()).not.toContain('on disk');
    panel.unmount();
  });
  it('ignores extension results from an earlier folder', async () => {
    let finish: (rows: unknown[]) => void = noop;
    vi.mocked(invoke).mockReset().mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    const panel = await mountPanel(ExtensionsPanel, { tree: current, hasAllocated: false, setQuery: noop });
    const stale = finish;
    panel.update({ tree: { ...current, id: 7 } });
    finish([{ extension: 'new', category: 'other', bytes: 1, allocated: 1, files: 1 }]); await panel.settle();
    stale([{ extension: 'stale', category: 'other', bytes: 2, allocated: 2, files: 1 }]); await panel.settle();
    expect(panel.html()).toContain('.new'); expect(panel.html()).not.toContain('.stale');
    panel.unmount();
  });

  it('persists age coloring, hides it without dates, and tolerates blocked storage', async () => {
    const storage = new Map<string, string>();
    vi.stubGlobal('localStorage', { getItem: (key: string) => storage.get(key), setItem: (key: string, value: string) => storage.set(key, value) });
    const panel = await mountPanel(ResultsScreen, { ...props, hasModified: true });
    panel.find('button', p => p.children === 'Age').props.onClick(); await panel.settle();
    expect(storage.get('sprawl.colorBy')).toBe('age');
    panel.update({ tree: current });
    expect(panel.all(Treemap)[0].props.colorBy).toBe('age');
    expect(panel.html()).toContain('File age legend'); expect(panel.html()).toContain('#a33a6b');
    expect(panel.html()).not.toContain('Colors show file type');
    panel.update({ hasModified: false });
    expect(panel.html()).not.toContain('Map colors'); expect(panel.html()).not.toContain('File age legend');
    panel.unmount();
    const restored = await mountPanel(ResultsScreen, { ...props, hasModified: true });
    expect(restored.find('button', p => p.children === 'Age').props['aria-pressed']).toBe(true);
    restored.find('button', p => p.children === 'Type').props.onClick(); await restored.settle();
    expect(storage.get('sprawl.colorBy')).toBe('type'); restored.unmount();
    vi.stubGlobal('localStorage', { getItem: () => { throw Error('blocked'); }, setItem: () => { throw Error('blocked'); } });
    const blocked = await mountPanel(ResultsScreen, { ...props, hasModified: true });
    blocked.find('button', p => p.children === 'Age').props.onClick(); await blocked.settle();
    expect(blocked.html()).toContain('File age legend'); blocked.unmount();
  });
  it('shows dates in file rows and file/folder tooltips only when known', () => {
    vi.stubGlobal('window', { devicePixelRatio: 1 });
    const dated = { ...props, hasModified: true, tree: { ...current, children: [{ ...current, id: 1, modified: file(1).modified }, { ...current, id: 2, kind: 'file' as const, modified: file(1).modified }] } };
    const html = renderToStaticMarkup(<ResultsScreen {...dated} />);
    expect(html).toContain('Newest file 12 Mar 2024'); expect(html).toContain('Modified 12 Mar 2024');
    expect(html).toContain('class="dated-path"><span>logs/file1.log</span><span>12 Mar 2024</span>');
    expect(renderToStaticMarkup(<ResultsScreen {...dated} hasModified={false} />)).not.toContain('12 Mar 2024');
  });
});
