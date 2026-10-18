import {readdir,readFile,stat} from 'node:fs/promises';
import {dirname,resolve} from 'node:path';
const repo='/home/andy/.local/share/meteogram-benchmark/repo',root=`${repo}/docs/benchmarks/2026-10-18-gfs-dictionary`;
const read=async p=>JSON.parse(await readFile(`${root}/${p}`,'utf8'));
const assert=(ok,m)=>{if(!ok)throw Error(m);};
const files=[];
async function walk(dir){for(const e of await readdir(dir,{withFileTypes:true}))e.isDirectory()?await walk(`${dir}/${e.name}`):files.push(`${dir}/${e.name}`);}
await walk(root);
let json=0,jsonl=0;
for(const f of files){if(f.endsWith('.json')){JSON.parse(await readFile(f,'utf8'));json++;}if(f.endsWith('.jsonl')){for(const s of (await readFile(f,'utf8')).split('\n').filter(Boolean))JSON.parse(s);jsonl++;}}
const s=await read('summary.json');
assert(s.runs.length===9&&s.variants.length===3,'Incomplete matrix');
assert(s.source.cells===24885&&s.source.leads.length===141&&s.source.levels.length===30&&s.source.variables.length===6&&s.source.surface_variables.length===2,'Source scope');
assert(s.source.cells*s.source.leads.length*(s.source.levels.length*s.source.variables.length+2)===638598870,'Float32 scope');
for(const v of s.variants){
  const runs=s.runs.filter(r=>r.variant===v.variant);
  const times=runs.map(r=>r.process.elapsed_seconds).sort((a,b)=>a-b);
  assert(runs.length===3&&times[1]===v.median_seconds,'Median/repeats');
  assert(runs.every(r=>r.process.exit_code===0&&r.load.candidate_merges_resumed&&r.load.finite_validation_cells===24885&&r.database_and_codebook_bytes===v.database_and_codebook_bytes),'Load gates');
  assert(v.queries.count===300&&v.queries.clients===1&&v.warmup_queries===300,'Query scope');
  const prefix=`runs/${v.variant}-r3`;
  const exact=await read(`${prefix}/exact.json`),http=await read(`${prefix}/http.json`);
  assert(exact.success&&http.success&&http.profiles.length===27,'Compatibility');
  for(const model of Object.values(exact.models))assert(model.complete_profiles_exact===100&&model.max_coordinate_error_degrees===0,'Exact profiles/coordinates');
}
const get=v=>s.variants.find(x=>x.variant===v);
const profileSaving=1-get('profile').database_and_codebook_bytes/get('raw').database_and_codebook_bytes;
const packedSaving=1-get('packed').database_and_codebook_bytes/get('raw').database_and_codebook_bytes;
assert((profileSaving*100).toFixed(1)==='43.9'&&(packedSaving*100).toFixed(1)==='49.9','Storage arithmetic');
assert((s.dictionary_preparation.max_rss_kib/1024).toFixed(2)==='310.11'&&s.dictionary_preparation.elapsed_seconds===24.81,'Encoding resources');
assert(s.stage.dictionary.preparation.dictionary.entries===61092751&&s.cardinality_comparison.success,'Palette scope');
const proof=await read('roundtrip-comparison.json');
assert(proof.success&&proof.mismatches===0&&proof.files_per_layout===565&&proof.float32_values_per_layout===638598870&&proof.compared_layouts.length===2,'Complete bit proof');
const a=await readFile(`${root}/runs/profile-r3/full-roundtrip.sha256`,'utf8'),b=await readFile(`${root}/runs/packed-r3/full-roundtrip.sha256`,'utf8');
assert(a===b&&a.trim().split('\n').length===565,'Full bit fingerprints');
const observer=await read('http-during-benchmark.json');
assert(observer.complete&&Object.values(observer.models).every(m=>m.errors===0&&m.successes===611),'Live service checks');
const preserve=await read('service-preservation.json'),cleanup=await read('cleanup.json');
assert(preserve.catalog_unchanged&&preserve.schedule_unchanged&&preserve.installed_binary_unchanged&&preserve.protected_rows_unchanged&&!preserve.service_restarted&&!preserve.installed_candidate,'Preservation');
assert(cleanup.dropped_owned_tables===19&&cleanup.remaining_experiment_tables===0&&cleanup.protected_tables===12,'Cleanup');
let links=0;
for(const rel of ['native/README.md','docs/plans/2026-10-18-other-model-load-benchmarks.md','docs/plans/2026-10-18-gfs-dictionary.md','docs/benchmarks/2026-10-18-gfs-dictionary/README.md']){
  const file=`${repo}/${rel}`;
  for(const match of (await readFile(file,'utf8')).matchAll(/\[[^\]]+\]\(([^)]+)\)/g)){
    const target=match[1].split('#')[0];
    if(!target||/^(https?:|mailto:)/.test(target))continue;
    await stat(resolve(dirname(file),target));links++;
  }
}
const result={time:Date.now()/1000,success:true,json_files:json,jsonl_files:jsonl,local_links:links,final_complete_loads:9,configurations:3,repeats:3,full_bit_values_per_dictionary_layout:638598870,full_bit_files_per_layout:565,profile_storage_reduction:profileSaving,packed_storage_reduction:packedSaving,encoding_peak_rss_mib:s.dictionary_preparation.max_rss_kib/1024,service_preserved:true,owned_cleanup_verified:true};
await Bun.write(`${root}/evidence-validation.json`,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result));
