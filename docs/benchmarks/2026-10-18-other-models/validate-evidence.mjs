import {readdir,readFile,stat} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
const repo='/home/andy/.local/share/meteogram-benchmark/repo';
const root=`${repo}/docs/benchmarks/2026-10-18-other-models`;
const files=[];
async function walk(dir){for(const e of await readdir(dir,{withFileTypes:true}))e.isDirectory()?await walk(`${dir}/${e.name}`):files.push(`${dir}/${e.name}`);}
await walk(root);
let json=0,jsonl=0;
for(const file of files){
  if(file.endsWith('.json')){JSON.parse(await readFile(file,'utf8'));json++;}
  if(file.endsWith('.jsonl')){for(const line of (await readFile(file,'utf8')).split('\n').filter(Boolean))JSON.parse(line);jsonl++;}
}
const read=async file=>JSON.parse(await readFile(`${root}/${file}`,'utf8'));
const s=await read('summary.json');
if(s.runs.length!==24||s.variants.length!==8)throw Error('Incomplete matrix');
const assert=(condition,msg)=>{if(!condition)throw Error(msg);};
for(const v of s.variants){
  const rs=s.runs.filter(r=>`${r.model}-${r.variant}-w${r.workers}`===v.label);
  assert(rs.length===3,'Repeat count');
  const times=rs.map(r=>r.process.elapsed_seconds).sort((a,b)=>a-b);
  assert(times[1]===v.median_seconds,'Median');
  assert(rs.every(r=>Number(r.parts.bytes_on_disk)===v.database_bytes),'Storage variance');
  assert(v.queries.count===300&&v.warmup_queries===300&&v.queries.clients===1,'Query scope');
  const prefix=`runs/${v.label}-r3`;
  const exact=await read(`${prefix}/exact.json`),http=await read(`${prefix}/http.json`);
  assert(exact.success&&http.success&&http.profiles.length===27,'Compatibility');
  for(const m of Object.values(exact.models))assert(m.complete_profiles_exact===100&&m.max_coordinate_error_degrees===0,'Reference comparison');
}
const get=label=>s.variants.find(v=>v.label===label);
const gfsSpeedup=get('gfs-raw-w1').median_seconds/get('gfs-raw-w4').median_seconds;
const aifsSpeedup=get('aifs-raw-w1').median_seconds/get('aifs-raw-w4').median_seconds;
const saving=1-get('aifs-packed-w4').database_and_codebook_bytes/get('aifs-raw-w4').database_and_codebook_bytes;
assert(gfsSpeedup.toFixed(2)==='2.54'&&aifsSpeedup.toFixed(2)==='2.13'&&(saving*100).toFixed(1)==='40.8','Report arithmetic');
const diversity=await read('gfs-cardinalities.json');
assert(diversity.reduce((n,f)=>n+f.entries,0)===61092751,'GFS cardinality');
assert(diversity.find(f=>f.field==='geopotential_height').entries===38867743,'GFS budget diagnosis');
const observer=await read('http-during-benchmark.json');
assert(observer.complete&&Object.values(observer.models).every(m=>m.errors===0&&m.successes===353),'Live observer');
const preserved=await read('service-preservation.json'),cleanup=await read('cleanup.json');
assert(preserved.catalog_unchanged&&preserved.schedule_unchanged&&preserved.installed_binary_unchanged&&preserved.protected_rows_unchanged&&!preserved.service_restarted&&!preserved.installed_candidate,'Preservation');
assert(cleanup.dropped_owned_tables===26&&cleanup.remaining_experiment_tables===0&&cleanup.failed_staging_temp_files_removed===564,'Cleanup');
let links=0;
for(const relative of ['native/README.md','docs/plans/2026-10-18-other-model-load-benchmarks.md','docs/benchmarks/2026-10-18-other-models/README.md']){
  const file=`${repo}/${relative}`,text=await readFile(file,'utf8');
  for(const match of text.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)){
    const target=match[1].split('#')[0];
    if(!target||/^(https?:|mailto:)/.test(target))continue;
    await stat(resolve(dirname(file),target));links++;
  }
}
const result={success:true,time:Date.now()/1000,json_files:json,jsonl_files:jsonl,local_links:links,full_loads:24,configurations:8,warmup_and_measured_queries_per_configuration:600,gfs_load_speedup:gfsSpeedup,aifs_raw_load_speedup:aifsSpeedup,aifs_tile_storage_reduction:saving,service_preserved:true,cleanup_verified:true};
await Bun.write(`${root}/evidence-validation.json`,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result));
