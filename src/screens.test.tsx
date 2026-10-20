import { renderToStaticMarkup } from 'react-dom/server';
import { Children, isValidElement, type ReactElement, type ReactNode } from 'react';
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
    const screen = HomeScreen({ drives: [drive], ready: true, start: async () => {}, pickFolder: async () => {}, saved: [scan, folder], openSaved, clearSaved, now });
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
    const props = { drives: [drive], ready: true, start: async () => {}, pickFolder: async () => {}, saved: [scan, folder], openSaved: async () => {}, clearSaved: async () => {}, now };
    for (const opening of [scan.path, folder.path]) {
      const html = renderToStaticMarkup(<HomeScreen {...props} opening={opening} />);
      const controls = html.match(/<button\b[^>]*>[^<]*<\/button>/g)!;
      expect(controls.filter(b => b.includes('Opening…'))).toHaveLength(1);
      expect(controls.every(b => b.includes('disabled=""'))).toBe(true);
    }
    expect(renderToStaticMarkup(<HomeScreen {...props} />)).not.toContain('disabled=""');
  });
  it('styles stale drive scans as warnings and fresh scans neutrally', () => {
    const props = { drives: [drive], ready: true, start: async () => {}, pickFolder: async () => {}, openSaved: async () => {}, clearSaved: async () => {}, now };
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
