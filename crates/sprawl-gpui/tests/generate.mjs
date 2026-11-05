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

// Pin both the clock and timezone for date-sensitive reference functions.
process.env.TZ = 'Etc/GMT-2';
const RealDate = Date;
const now = RealDate.UTC(2026, 5, 15, 12, 0);
globalThis.Date = class extends RealDate {
  constructor(...args) { super(...(args.length ? args : [now])); }
  static now() { return now; }
};

const deltas = [-1073741824, -1, 0, 1, 1536, 1073741824].map(value=>({value,result:model.formatDelta(value)}));
const times = [0, RealDate.UTC(2025,0,2), RealDate.UTC(2026,5,15,23,30), now];
const dates = times.map(value=>({value,result:model.formatDate(value)}));
const scanDates = times.map(value=>({value,result:model.formatScanDate(value)}));
const ages = [-1,0,1,59,60,1439,1440,2880,10079,10080,43199,43200,525599,525600,1051200].map(minutes=>({value:now-minutes*60000,result:model.formatAge(now-minutes*60000,now)}));
const ageColors = [0,now+1,...[0,29,30,179,180,364,365,729,730,1824,1825,5000].map(days=>now-days*86400000)].map(value=>({value,result:model.ageColor(value,now)}));
const gaps = [[100*1024**3,10*1024**3,80*1024**3],[100*1024**3,10*1024**3,90*1024**3],[1000*1024**3,0,995*1024**3],[1000*1024**3,0,990*1024**3],[1024,0,0],[100,0,200]].map(([total,free,scanned])=>({total,free,scanned,result:model.notFoundByScan(total,free,scanned)}));
const roots = [['C:\\','c:/'],['C:\\folder','C:\\'],['/mnt/data///','/mnt/data'],['D:','d:\\']].map(([path,mount])=>({path,mount,result:model.isDriveRoot(path,mount)}));
const drive = {mount:'X:\\',label:'Fixture',total:100*1024**3,free:20*1024**3};
const stale = [0,7*86400000,7*86400000+1].flatMap(elapsed=>[null,drive,{...drive,free:21*1024**3},{...drive,free:19*1024**3}].map(drive=> {
  const saved = {version:5,path:'X:\\fixture',scannedAt:now-elapsed,size:0,files:0,folders:0,errors:0,driveFree:20*1024**3,driveTotal:100*1024**3};
  return {saved,drive,result:model.staleness(saved,drive ?? undefined,now)};
}));
writeFileSync(`${out}/results.json`,JSON.stringify({now,offset:7200,deltas,dates,scanDates,ages,ageColors,gaps,roots,stale},null,2)+'\n');
globalThis.Date = RealDate;

const trailRoot = dir(0, []), a = dir(1, []), b = dir(2, []), c = dir(3, []);
const trailCases = [
  { trail: [trailRoot, a, b], history: [[trailRoot], [trailRoot, c]], ids: [0,1,2,3], nodes: [{...trailRoot,size:90},{...a,size:50},null,c] },
  { trail: [trailRoot, a], history: [[trailRoot,a],[trailRoot,c]], ids: [0,1,3], nodes: [trailRoot,null,c] },
  { trail: [trailRoot,a], history: [[trailRoot]], ids: [0,1], nodes: [null,a] },
  { trail: [trailRoot,a], history: [[trailRoot,a]], ids: [0,1], nodes: [{...trailRoot,size:100},{...a,size:80}] },
  { trail: [trailRoot,{...a,id:null}], history: [[trailRoot,a]], ids: [0], nodes: [trailRoot] },
];
writeFileSync(`${out}/trails.json`, JSON.stringify(trailCases.map(c=>({...c,result:model.refreshTrails(c.trail,c.history,c.ids,c.nodes)})),null,2)+'\n');
