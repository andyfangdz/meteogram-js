import {mkdir,copyFile,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const b='/home/andy/.local/share/meteogram-gfs-dictionary',main='/home/andy/.local/share/meteogram-native';
const j=async p=>JSON.parse(await Bun.file(p).text());
const save=async (name,data)=>Bun.write(`${b}/${name}`,JSON.stringify(data,null,2)+'\n');
const query=async sql=>{const r=await fetch('http://127.0.0.1:18123',{method:'POST',body:sql});const t=await r.text();if(!r.ok)throw Error(t);return t;};
const catalog=await j(`${main}/catalog.json`);
await copyFile(`${main}/catalog.json`,`${b}/main-catalog-before.json`);
await copyFile(`${main}/schedule.json`,`${b}/main-schedule-before.json`);
await Bun.write(`${b}/protected-rows-before.jsonl`,await query("SELECT database,table,sum(rows) AS rows FROM system.parts WHERE active AND database IN ('meteogram_native','meteogram_bench') AND match(table,'^(hrrr|gfs|aifs)_') GROUP BY database,table ORDER BY database,table FORMAT JSONEachRow"));
const installed=createHash('sha256').update(await readFile('/home/andy/.local/bin/meteogram-native')).digest('hex');
await save('installed-before.json',{time:Date.now()/1000,sha256:installed});
const source=catalog.current.gfs;
for(const id of [source.database,source.table])if(!/^[a-z][a-z0-9_]+$/.test(id))throw Error('Invalid identifier');
const table='bench_gfs_dict_source_20261018_06';
await query(`CREATE TABLE meteogram_native.${table} CLONE AS ${source.database}.${source.table}`);
await query(`SYSTEM STOP MERGES meteogram_native.${table}`);
const reference=structuredClone(catalog);
reference.previous={};reference.retired=[];
for(const [model,entry] of Object.entries(reference.current)){
  const original=catalog.current[model];
  if(model==='gfs')Object.assign(entry,{revision:table,table,database:'meteogram_native',owned:true});else entry.owned=false;
  const dir=`${b}/reference/revisions/${entry.revision}`;
  await mkdir(dir,{recursive:true});
  for(const name of ['metadata.json','cells.bin.zst','values.dict.zst'])if(await Bun.file(`${main}/revisions/${original.revision}/${name}`).exists())await copyFile(`${main}/revisions/${original.revision}/${name}`,`${dir}/${name}`);
}
await save('reference/catalog.json',reference);
await save('source.json',{time:Date.now()/1000,table,source,metadata:await j(`${b}/reference/revisions/${table}/metadata.json`)});
console.log(JSON.stringify({source_table:source.table,clone:table,catalog_generation:catalog.generation}));
