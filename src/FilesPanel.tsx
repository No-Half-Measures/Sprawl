import { t } from './i18n';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { categoryColor, count, formatSize, percent, type ExtensionTotal, type FilePage, type FileSort, type LargeFile, type TreeNode } from './model';

interface FilesProps {
  tree: TreeNode;
  hasModified: boolean;
  renderFile: (file: LargeFile, index: number) => ReactNode;
}

export default function FilesPanel({ tree, hasModified, renderFile }: FilesProps) {
  const [filter, setFilter] = useState('');
  const [sort, setSort] = useState<FileSort>('size');
  const [descending, setDescending] = useState(true);
  const [page, setPage] = useState<FilePage | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const generation = useRef(0);
  const query = filter.trim();
  const activeSort = sort === 'modified' && !hasModified ? 'size' : sort;
  async function load(offset: number, version: number) {
    setBusy(true);
    setError('');
    try {
      const next = await invoke<FilePage>('get_files', { nodeId: tree.id, query, sort: activeSort, descending, offset, limit: 200 });
      if (generation.current === version) setPage(previous => ({ total: next.total, files: offset ? [...(previous?.files ?? []), ...next.files] : next.files }));
    } catch (e) {
      if (generation.current === version) setError(String(e));
    } finally {
      if (generation.current === version) setBusy(false);
    }
  }
  useEffect(() => {
    const version = ++generation.current;
    setPage(null);
    setError('');
    setBusy(true);
    const timeout = window.setTimeout(() => void load(0, version), query ? 200 : 0);
    return () => { generation.current++; window.clearTimeout(timeout); };
  }, [tree, query, activeSort, descending]);
  function changeSort(next: FileSort) {
    setSort(next);
    setDescending(next === activeSort ? !descending : next === 'size');
  }
  return <>
    <div className="files-toolbar">
      <div className="search-field">
        <input type="search" aria-label={t('files.filter')} placeholder={t('files.placeholder')} value={filter} spellCheck={false}
          aria-invalid={error ? true : undefined} aria-describedby={error ? 'files-error' : undefined}
          onChange={e => setFilter(e.target.value)} />
        {error && <small id="files-error" className="search-error">{error}</small>}
      </div>
      <div className="file-sorts" role="group" aria-label={t('files.sort')}>
        {(['size', 'name', ...(hasModified ? ['modified'] : [])] as FileSort[]).map(value => <button key={value}
          aria-pressed={activeSort === value} onClick={() => changeSort(value)}>
          {t(`sort.${value}`)}{activeSort === value && (descending ? ' \u2193' : ' \u2191')}
        </button>)}
      </div>
      <span className="files-total">{page ? count(page.total, 'file') : busy ? t('common.loading') : ''}</span>
    </div>
    <div className="largest-scroll" aria-busy={busy}>
      {page?.files.map(renderFile)}
      {page && !page.files.length && <p className="quiet-empty">{t('files.none')}</p>}
      {page && page.files.length < page.total && <button className="show-more" disabled={busy} onClick={() => void load(page.files.length, generation.current)}>{busy ? t('common.loading') : t('files.more')}</button>}
    </div>
  </>;
}

export function ExtensionsPanel({ tree, hasAllocated, setQuery }: { tree: TreeNode; hasAllocated: boolean; setQuery: (query: string) => void }) {
  const [rows, setRows] = useState<ExtensionTotal[] | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    let stale = false;
    setRows(null);
    setError('');
    invoke<ExtensionTotal[]>('get_extension_breakdown', { nodeId: tree.id }).then(next => {
      if (!stale) setRows(next);
    }).catch(e => { if (!stale) setError(String(e)); });
    return () => { stale = true; };
  }, [tree]);
  return <div className="largest-scroll">
    {rows?.map(row => {
      const content = <>
        <span className="row-name"><i className="extension-swatch" style={{ background: categoryColor(row.category) }} /><span>{row.extension ? '.' + row.extension : t('files.noExtension')}</span></span>
        <span className="row-data"><span>{count(row.files, 'file')}</span><strong className="row-size" title={hasAllocated ? t('results.onDisk', { size: formatSize(row.allocated) }) : undefined}>{formatSize(row.bytes)}</strong><span className="row-percent">{percent(row.bytes, tree.size)}</span></span>
        <span className="row-meter"><i style={{ width: (tree.size ? row.bytes / tree.size * 100 : 0) + '%', background: categoryColor(row.category) }} /></span>
      </>;
      return row.extension ? <button key={row.extension} className="content-row" onClick={() => setQuery('*.' + row.extension)}>{content}</button>
        : <div key="" className="content-row">{content}</div>;
    })}
    {error && <p className="quiet-empty" role="status">{error}</p>}
    {!rows && !error && <p className="quiet-empty">{t('files.loadingExtensions')}</p>}
    {rows && !rows.length && <p className="quiet-empty">{t('files.none')}</p>}
  </div>;
}
