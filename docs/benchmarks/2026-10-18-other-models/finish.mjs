import {readdir,readFile,copyFile,unlink} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const base='/home/andy/.local/share/meteogram-other-models';
const main='/home/andy/.local/share/meteogram-native';
const j=async p=>JSON.parse(await Bun.file(p).text());
const write=async (name,data)=>Bun.write(`${base}/${name}`,JSON.stringify(data,null,2)+'\n');
const query=async sql=>{
  const response=await fetch('http://127.0.0.1:18123',{method:'POST',body:sql});
  const text=await response.text();
  if(!response.ok)throw Error(text);
  return text;
};
const sha=async p=>createHash('sha256').update(await readFile(p)).digest('hex');
if(!(await j(`${base}/verification-complete.json`)).complete)throw Error('Verification incomplete');
if(!(await j(`${base}/main-final-http.json`)).success)throw Error('Main HTTP check failed');
const before=await j(`${base}/main-catalog-before.json`);
const catalog=await j(`${main}/catalog.json`),schedule=await j(`${main}/schedule.json`);
if(JSON.stringify(catalog)!==JSON.stringify(before))throw Error('Main catalog changed');
if(JSON.stringify(schedule)!==JSON.stringify(await j(`${base}/main-schedule-before.json`)))throw Error('Main schedule changed');
await copyFile(`${main}/catalog.json`,`${base}/main-catalog-after.json`);
await copyFile(`${main}/schedule.json`,`${base}/main-schedule-after.json`);
const expectedInstalled=(await Bun.file(`${base}/installed-baseline.sha256`).text()).split(/\s/)[0];
const expectedCandidate=(await Bun.file(`${base}/candidate.sha256`).text()).split(/\s/)[0];
const installedHash=await sha('/home/andy/.local/bin/meteogram-native');
const candidateHash=await sha(`${base}/candidate`);
if(expectedInstalled!==installedHash || expectedCandidate!==candidateHash)throw Error('Executable changed');
const protectedRows=(await Bun.file(`${base}/protected-rows-before.jsonl`).text()).trim().split('\n').map(JSON.parse);
const protectedSet=new Set(protectedRows.map(r=>`${r.database}.${r.table}`));
for(const group of [catalog.current,catalog.previous])for(const e of Object.values(group))protectedSet.add(`${e.database}.${e.table}`);
for(const e of catalog.retired)protectedSet.add(`${e.database}.${e.table}`);
const tables=[];
for(const label of await readdir(`${base}/runs`))tables.push((await j(`${base}/runs/${label}/run.json`)).table);
for(const e of Object.values((await j(`${base}/reference/catalog.json`)).current))if(e.owned)tables.push(e.table);
if(tables.length!==26 || new Set(tables).size!==26)throw Error('Unexpected experiment ownership list');
for(const table of tables)if(!/^bench_models_(gfs|aifs)_[a-z0-9_]+$/.test(table)||protectedSet.has(`meteogram_native.${table}`))throw Error(`Unsafe cleanup ${table}`);
await write('cleanup-plan.json',{time:Date.now()/1000,owned_tables:tables,protected_tables:[...protectedSet].sort(),main_catalog_unchanged:true});
for(const table of tables)await query(`DROP TABLE meteogram_native.${table} SYNC`);
let removedWeatherFiles=0;
const weatherRoots=['raw-gfs','raw-aifs','sorted-aifs','dictionary-aifs','dictionary-gfs-failed','raw-gfs-failed'];
for(const dir of weatherRoots)for(const name of await readdir(`${base}/${dir}`)) {
  if(/^(lead|surface)-[0-9]{3}\.stage\.(json|zst)$/.test(name)) {
    await unlink(`${base}/${dir}/${name}`);
    removedWeatherFiles++;
  }
}
for(const label of await readdir(`${base}/runs`))if(label.endsWith('-r3'))for(const name of await readdir(`${base}/runs/${label}`))if(/^(lead|surface)-[0-9]{3}\.stage\.(json|zst)$/.test(name))await unlink(`${base}/runs/${label}/${name}`);
const afterText=await query("SELECT database,table,sum(rows) AS rows FROM system.parts WHERE active AND database IN ('meteogram_native','meteogram_bench') GROUP BY database,table ORDER BY database,table FORMAT JSONEachRow");
await Bun.write(`${base}/protected-rows-after.jsonl`,afterText);
const after=afterText.trim().split('\n').map(JSON.parse);
for(const row of protectedRows) {
  const found=after.find(r=>r.database===row.database&&r.table===row.table);
  if(!found||Number(found.rows)!==Number(row.rows))throw Error(`Protected rows changed ${row.table}`);
}
const remaining=await query("SELECT name FROM system.tables WHERE database='meteogram_native' AND startsWith(name,'bench_models_') FORMAT JSONEachRow");
if(remaining.trim())throw Error('Experiment tables remain');
await Bun.write(`${base}/remaining-experiment-tables.jsonl`,remaining);
const response=await fetch('http://127.0.0.1:18080/health');
const health=await response.json();
if(!response.ok)throw Error('Main health failed');
await write('main-final-health.json',health);
await write('cleanup.json',{time:Date.now()/1000,dropped_owned_tables:tables.length,removed_weather_files:removedWeatherFiles,remaining_experiment_tables:0,protected_table_count:protectedRows.length,protected_rows_unchanged:true});
await write('service-preservation.json',{time:Date.now()/1000,catalog_unchanged:true,catalog_generation:catalog.generation,schedule_unchanged:true,next_update:schedule.next_update,installed_binary_unchanged:true,installed_sha256:installedHash,candidate_sha256:candidateHash,candidate_binary_unchanged:true,protected_tables:protectedRows.length,protected_rows_unchanged:true,service_restarted:false,installed_candidate:false});
await write('bench-finished.json',{time:Date.now()/1000,complete:true});
console.log('Cleaned 26 owned experiment tables; protected rows, installed binary, catalog, schedule and API preserved.');
