import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import HomeScreen from './HomeScreen';
import ResultsScreen from './ResultsScreen';
import { categories, formatSize, percent, type TreeNode } from './model';

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
    expect(html).toContain(`<strong>${formatSize(current.size)}</strong>`);
    expect(html).not.toContain('% of C:');
  });

  it('keeps five drive cards and a single-line home heading', () => {
    const html = renderToStaticMarkup(
      <HomeScreen
        drives={['C','D','E','G','H'].map(letter => ({ mount: `${letter}:\\`, label: 'Local disk', total: 1024, free: 512 }))}
        ready
        pickFolder={async () => {}}
        start={async () => {}}
      />,
    );
    expect(html.match(/class="drive-card"/g)).toHaveLength(5);
    expect(html).toContain('<h1>See where your space goes.</h1>');
  });
});
