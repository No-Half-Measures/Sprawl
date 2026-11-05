// Run from the repository root: node crates/sprawl-gpui/tests/generate.mjs
import ts from 'typescript';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
const out = resolve('crates/sprawl-gpui/tests/fixtures');
mkdirSync(out, { recursive: true });
// Transpile the actual source, keeping the real d3 import and category data.
async function source(file) {
  const text = readFileSync(file, 'utf8').replace("import categories from './categories.json';", `const categories = ${readFileSync('src/categories.json', 'utf8')};`);
  const js = ts.transpile(text, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext });
  const target = resolve('crates/sprawl-gpui/tests', `.generated-${file.split('/').pop()}.mjs`);
  writeFileSync(target, js);
  const module = await import(pathToFileURL(target));
  const { unlinkSync } = await import('node:fs');
  unlinkSync(target);
  return module;
}
const layout = await source('src/layout.ts');
const model = await source('src/model.ts');
const file = (id, size, name = `file-${id}.txt`) => ({ id, size, name, kind: 'file', files: 1, folders: 0, allocated: size, category: 'documents' });
const dir = (id, children) => ({ id, name: `folder-${id}`, kind: 'dir', size: children.reduce((s,n)=>s+n.size,0), files: children.reduce((s,n)=>s+n.files,0), folders: 1, allocated: 0, children });
const trees = [
  ['nested',dir(0,[dir(1,[dir(2,[file(3,80),file(4,20)]),file(5,70)]),file(6,65)])],
  ['single',dir(0,[dir(1,[file(2,100)])])],
  ['zero',dir(0,[dir(1,[file(2,0),file(3,10)]),file(4,0),file(5,20)])],
  ['aggregate',dir(0,[dir(1,[file(2,40),{...file(null,30),kind:'other',name:'12 smaller items'}]),file(3,20)])],
  ['tiny-files',dir(0,[file(1,100000),...Array.from({length:130},(_,i)=>file(i+2,i%4))])],
];
const cases = [];
for (const [name,tree] of trees) for (const [width,height] of [[1000,700],[4,3],[137,89]]) for (const frames of [true,false]) cases.push({name,width,height,frames,tree,tiles:layout.layoutTree(tree,width,height,frames)});
cases.push({name:'empty',width:100,height:100,frames:true,tree:dir(0,[file(1,0)]),tiles:[]});
writeFileSync(`${out}/layout.json`,JSON.stringify(cases,null,2)+'\n');
const measure = s => Array.from(s).length * 7;
const labels = ['a','abcd','abcde','a long file-name_with.words.txt','😀😀😀😀😀😀😀.txt','folder\\some-name.ext'];
const labelCases = labels.flatMap(name=>[0,21,35,49,98,500].flatMap(width=>[0,1,2].map(lines=>({name,width,lines,fit:layout.fitLabel(name,width,measure),wrap:layout.wrapLabel(name,width,lines,measure)}))));
const colors = ['#ffffff','#000000','#94a3b5','#86b9ef','#354557','#ff0000','#00ff00','#0000ff'].map(hex=>({hex,result:layout.textColor(hex)}));
const sizes = [0,-1,1,999,1023,1024,1536,1024**2*99.95,1024**2*100.5,1024**5,1024**6].map(value=>({value,result:model.formatSize(value)}));
const shares = [[0,0],[1,10000],[1,1000],[12345,99999],[1,3],[100,100]].map(([value,total])=>({value,total,result:model.percent(value,total)}));
const counts = [0,1,2,1234567].map(value=>({value,result:model.count(value,'file')}));
const paths = [['short',100],['abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ',20],['abcdef',0]].map(([value,limit])=>({value,limit,result:model.middlePath(value,limit)}));
writeFileSync(`${out}/format.json`,JSON.stringify({labels:labelCases,colors,sizes,shares,counts,paths},null,2)+'\n');
