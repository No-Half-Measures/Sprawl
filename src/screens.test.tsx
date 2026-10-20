import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import HomeScreen from './HomeScreen';
import ResultsScreen from './ResultsScreen';
import { categories, type TreeNode } from './model';

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
    expect(html).toContain('aria-label="Up one folder (Alt+↑)"');
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
