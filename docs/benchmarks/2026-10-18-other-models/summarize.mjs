import {readdir, stat} from 'node:fs/promises';
const base='/home/andy/.local/share/meteogram-other-models';
const read=async p=>JSON.parse(await Bun.file(`${base}/${p}`).text());
const jsonl=async p=>(await Bun.file(`${base}/${p}`).text()).trim().split('\n').filter(Boolean).map(JSON.parse);
const variants=new Map();
const runs=[];
for(const label of (await readdir(`${base}/runs`)).sort()) {
  const prefix=`runs/${label}`;
  const run=await read(`${prefix}/run.json`);
  const process=await read(`${prefix}/process.json`);
  const load=await read(`${prefix}/load.json`);
  const validation=await read(`${prefix}/validation.json`);
  const parts=(await jsonl(`${prefix}/parts.jsonl`))[0];
  const meta=await read(`${prefix}/metadata.json`);
  const ranges=await jsonl(`${prefix}/range-queries.jsonl`);
  const parsed=ranges.map(r=>{
    const match=r.query.match(/^SELECT toUInt8\(count\(\)=(\d+) AND min\((cell_id|tile_id)\)=(\d+) AND max\(\2\)=(\d+)/);
    if(!match)throw Error(`Unrecognized range ${label}`);
    return {query_id:r.query_id,expected_rows:Number(match[1]),key:match[2],min:Number(match[3]),max:Number(match[4]),read_rows:Number(r.read_rows),read_bytes:Number(r.read_bytes),query_duration_ms:Number(r.query_duration_ms),memory_usage:Number(r.memory_usage)};
  }).sort((a,b)=>a.min-b.min);
  const expected=Number(load.storage_rows);
  if(new Set(parsed.map(r=>r.query_id)).size!==parsed.length || parsed.reduce((s,r)=>s+r.expected_rows,0)!==expected || parsed.some((r,i)=>r.expected_rows!==r.read_rows || (i>0 && r.min<=parsed[i-1].max)))throw Error(`Range coverage ${label}`);
  if(process.exit_code!==0 || load.candidate_merges_resumed!==true || Number(parts.storage_rows)!==expected || Number(load.finite_validation_cells)!==24885)throw Error(`Load proof ${label}`);
  const book=Number(meta.storage?.dictionary?.encoded_bytes??0);
  const cells=meta.cells,leads=meta.leads.length,levels=meta.levels.length;
  const values=cells*leads*(levels*meta.variables.length+meta.surface_variables.length);
  const entry={...run,process,load,validation,parts,codebook_bytes:book,database_and_codebook_bytes:Number(parts.bytes_on_disk)+book,range_coverage:{queries:parsed.length,validated_physical_rows:expected,validated_cells:cells,logical_cell_lead_rows:cells*leads,full_float32_values:values,overlap:false,ranges:parsed}};
  runs.push(entry);
  const key=`${run.model}-${run.variant}-w${run.workers}`;
  if(!variants.has(key))variants.set(key,[]);
  variants.get(key).push(entry);
  await Bun.write(`${base}/${prefix}/range-summary.json`,JSON.stringify(entry.range_coverage,null,2)+'\n');
}
const summaries=[];
for(const [label,entries] of variants) {
  entries.sort((a,b)=>a.repeat-b.repeat);
  if(entries.length!==3)throw Error(`Repeats ${label}`);
  const elapsed=entries.map(e=>e.process.elapsed_seconds),sorted=[...elapsed].sort((a,b)=>a-b);
  const final=entries.find(e=>e.repeat===3);
  const p=`runs/${label}-r3`;
  const queries=await read(`${p}/queries.json`);
  const exact=await read(`${p}/exact.json`),http=await read(`${p}/http.json`),audit=await read(`${p}/independent-audit.json`);
  if(!exact.success||!http.success||http.profiles.length!==27||exact.models[final.model].complete_profiles_exact!==100)throw Error(`Verification ${label}`);
  summaries.push({label,model:final.model,variant:final.variant,workers:final.workers,seconds_by_repeat:elapsed,median_seconds:sorted[1],min_seconds:sorted[0],max_seconds:sorted[2],client_max_rss_kib:Math.max(...entries.map(e=>e.process.max_rss_kib)),database_bytes:Number(final.parts.bytes_on_disk),codebook_bytes:final.codebook_bytes,database_and_codebook_bytes:final.database_and_codebook_bytes,physical_rows:Number(final.parts.storage_rows),cells:24885,logical_rows:final.load.logical_rows,warmup_queries:queries.warmup_queries_per_model,queries:queries.models[final.model],independent_audit:audit,exact:exact.models[final.model],http:{success:http.success,compared_profiles:http.profiles.length,invalid_inputs:http.invalid_inputs,outside_region:http.outside_region,gzip_bytes:http.gzip_bytes}});
}
const sources={};
for(const model of ['gfs','aifs']) {
  const m=await read(`source-${model}/metadata.json`);
  sources[model]={model,cells:m.cells,leads:m.leads,levels:m.levels,variables:m.variables,surface_variables:m.surface_variables,init_time:m.init_time,snapshot_id:m.snapshot_id,dataset:m.dataset,retained_region:{latitude:[24,50],longitude:[-125,-66]},float32_values:m.cells*m.leads.length*(m.levels.length*m.variables.length+m.surface_variables.length)};
}
const stage={};
for(const name of ['raw-gfs','raw-aifs','sorted-aifs']) {
  const names=(await readdir(`${base}/${name}`)).filter(n=>n.endsWith('.stage.zst'));
  let bytes=0;
  for(const file of names)bytes+=(await stat(`${base}/${name}/${file}`)).size;
  stage[name]={weather_files:names.length,weather_stage_bytes:bytes,preparation:await read(`${name==='raw-aifs'?'dictionary-aifs':name}/preparation.json`)};
}
const summary={time:Date.now()/1000,scope:'Complete fresh-table staged loads on the same ARM Linux host; all ingestion gates inside CLI clock. Preparation/acquisition excluded. Warm complete-profile database queries are a separate loopback measurement.',source_pins:sources,stage,variants:summaries,runs:runs.map(({range_coverage,validation,...r})=>({...r,range_coverage:{...range_coverage,ranges:undefined}})),gfs_dictionary:{complete:false,reason:'Existing MAX_FIELD_ENTRIES=32000000 guard rejects geopotential palettes; no full dictionary/tile load measured.',preparation:await read('gfs-prepare-process.json'),field_cardinalities:await read('gfs-cardinalities.json')}};
await Bun.write(`${base}/summary.json`,JSON.stringify(summary,null,2)+'\n');
console.log(JSON.stringify(summaries.map(({label,seconds_by_repeat,median_seconds,database_and_codebook_bytes,queries})=>({label,seconds_by_repeat,median_seconds,database_and_codebook_bytes,p50_ms:queries.p50_ms,p95_ms:queries.p95_ms})),null,2));
