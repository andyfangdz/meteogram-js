import {readdir,readFile,copyFile,unlink,link,stat} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const b='/home/andy/.local/share/meteogram-gfs-dictionary',main='/home/andy/.local/share/meteogram-native';
const read=async p=>JSON.parse(await Bun.file(p).text());
const save=async(n,v)=>Bun.write(`${b}/${n}`,JSON.stringify(v,null,2)+'\n');
const hash=async p=>createHash('sha256').update(await readFile(p)).digest('hex');
const query=async sql=>{const r=await fetch('http://127.0.0.1:18123',{method:'POST',body:sql});const t=await r.text();if(!r.ok)throw Error(t);return t;};
if(!(await read(`${b}/verification-complete.json`)).complete||!(await read(`${b}/roundtrip-comparison.json`)).success)throw Error('Verification incomplete');
if(!(await read(`${b}/main-final-http.json`)).success)throw Error('Main HTTP verification failed');
const catalog=await read(`${main}/catalog.json`),schedule=await read(`${main}/schedule.json`);
if(JSON.stringify(catalog)!==JSON.stringify(await read(`${b}/main-catalog-before.json`))||JSON.stringify(schedule)!==JSON.stringify(await read(`${b}/main-schedule-before.json`)))throw Error('Main state changed');
const installed=await hash('/home/andy/.local/bin/meteogram-native');
if(installed!==(await read(`${b}/installed-before.json`)).sha256)throw Error('Installed binary changed');
const candidate=await hash(`${b}/candidate`);
if(candidate!==(await Bun.file(`${b}/candidate.sha256`).text()).split(/\s/)[0])throw Error('Candidate binary changed');
const before=(await Bun.file(`${b}/protected-rows-before.jsonl`).text()).trim().split('\n').map(JSON.parse);
if(before.length!==12)throw Error('Unexpected protected scope');
const protectedTables=new Set(before.map(r=>`${r.database}.${r.table}`));
for(const group of [catalog.current,catalog.previous])for(const e of Object.values(group))protectedTables.add(`${e.database}.${e.table}`);
for(const e of catalog.retired)protectedTables.add(`${e.database}.${e.table}`);
const tables=[(await read(`${b}/source.json`)).table];
for(const root of [`${b}/runs`,`${b}/before-projection/runs`])for(const name of await readdir(root))tables.push((await read(`${root}/${name}/run.json`)).table);
if(tables.length!==19||new Set(tables).size!==19)throw Error('Unexpected ownership list');
for(const table of tables)if(!/^bench_gfs_dict_[a-z0-9_]+$/.test(table)||protectedTables.has(`meteogram_native.${table}`))throw Error(`Unsafe cleanup ${table}`);
await save('cleanup-plan.json',{time:Date.now()/1000,owned_tables:tables,protected_tables:[...protectedTables].sort()});
for(const table of tables)await query(`DROP TABLE meteogram_native.${table} SYNC`);
let removedWeather=0;
for(const root of [`${b}/raw`,`${b}/dictionary`,`${b}/roundtrip-profile`,`${b}/roundtrip-packed`,`${b}/before-projection/roundtrip-profile`,`${b}/before-projection/roundtrip-packed`])for(const name of await readdir(root))if(/^(lead|surface)-[0-9]{3}\.stage\.(json|zst)(\.tmp)?$/.test(name)){await unlink(`${root}/${name}`);removedWeather++;}
for(const root of [`${b}/runs`,`${b}/before-projection/runs`])for(const name of await readdir(root))for(const file of await readdir(`${root}/${name}`))if(/^(lead|surface)-[0-9]{3}\.stage\.(json|zst)$/.test(file))await unlink(`${root}/${name}/${file}`);
// Retain each descriptor's required immutable book, sharing identical bytes.
const book=`${b}/dictionary/values.dict.zst`,bookHash=await hash(book),bookStat=await stat(book);
let deduplicatedBooks=0;
for(const root of [`${b}/runs`,`${b}/before-projection/runs`])for(const name of await readdir(root)){
  const file=`${root}/${name}/values.dict.zst`;
  if(!(await Bun.file(file).exists()))continue;
  if(await hash(file)!==bookHash)throw Error('Codebook identity changed');
  if((await stat(file)).ino!==bookStat.ino){await unlink(file);await link(book,file);deduplicatedBooks++;}
}
const afterText=await query("SELECT database,table,sum(rows) AS rows FROM system.parts WHERE active AND database IN ('meteogram_native','meteogram_bench') AND match(table,'^(hrrr|gfs|aifs)_') GROUP BY database,table ORDER BY database,table FORMAT JSONEachRow");
await Bun.write(`${b}/protected-rows-after.jsonl`,afterText);
const after=afterText.trim().split('\n').map(JSON.parse);
if(after.length!==12||before.some(r=>!after.some(a=>a.database===r.database&&a.table===r.table&&Number(a.rows)===Number(r.rows))))throw Error('Protected rows changed');
const remaining=await query("SELECT name FROM system.tables WHERE database='meteogram_native' AND startsWith(name,'bench_gfs_dict_') FORMAT JSONEachRow");
if(remaining.trim())throw Error('Experiment tables remain');
await Bun.write(`${b}/remaining-experiment-tables.jsonl`,remaining);
await copyFile(`${main}/catalog.json`,`${b}/main-catalog-after.json`);
await copyFile(`${main}/schedule.json`,`${b}/main-schedule-after.json`);
const response=await fetch('http://127.0.0.1:18080/health'),health=await response.json();
if(!response.ok||Object.values(health.models).some(m=>!m.available))throw Error('Main unhealthy');
await save('main-final-health.json',health);
await save('cleanup.json',{time:Date.now()/1000,dropped_owned_tables:tables.length,removed_weather_files:removedWeather,deduplicated_codebooks:deduplicatedBooks,remaining_experiment_tables:0,protected_tables:12,protected_rows_unchanged:true,retained_codebook_sha256:bookHash});
await save('service-preservation.json',{time:Date.now()/1000,catalog_unchanged:true,catalog_generation:catalog.generation,schedule_unchanged:true,next_update:schedule.next_update,installed_binary_unchanged:true,installed_sha256:installed,candidate_sha256:candidate,candidate_binary_unchanged:true,protected_tables:12,protected_rows_unchanged:true,service_restarted:false,installed_candidate:false});
await save('bench-finished.json',{time:Date.now()/1000,complete:true});
console.log('Removed 19 owned experiment tables; main service/catalog/schedule/binary and all 12 protected tables preserved.');
