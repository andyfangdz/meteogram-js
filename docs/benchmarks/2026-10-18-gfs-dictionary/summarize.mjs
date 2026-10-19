import {readdir,stat} from 'node:fs/promises';
const b='/home/andy/.local/share/meteogram-gfs-dictionary';
const read=async name=>JSON.parse(await Bun.file(`${b}/${name}`).text());
const lines=async name=>(await Bun.file(`${b}/${name}`).text()).trim().split('\n').filter(Boolean).map(JSON.parse);
const runs=[];
for(const name of (await readdir(`${b}/runs`)).sort()){
  const root=`runs/${name}`;
  const run=await read(`${root}/run.json`),process=await read(`${root}/process.json`),load=await read(`${root}/load.json`),validation=await read(`${root}/validation.json`),metadata=await read(`${root}/metadata.json`),parts=(await lines(`${root}/parts.jsonl`))[0],cache=await read(`${root}/input-cache.json`);
  const ranges=(await lines(`${root}/range-queries.jsonl`)).map(q=>{
    const m=q.query.match(/^SELECT toUInt8\(count\(\)=(\d+) AND min\((cell_id|tile_id)\)=(\d+) AND max\(\2\)=(\d+)/);
    if(!m)throw Error(`Unrecognized range ${name}`);
    return{query_id:q.query_id,key:m[2],expected_rows:Number(m[1]),min:Number(m[3]),max:Number(m[4]),read_rows:Number(q.read_rows),read_bytes:Number(q.read_bytes),query_duration_ms:Number(q.query_duration_ms),memory_usage:Number(q.memory_usage)};
  }).sort((a,c)=>a.min-c.min);
  const rows=Number(parts.storage_rows);
  if(process.exit_code!==0||load.candidate_merges_resumed!==true||load.finite_validation_cells!==24885||cache.errors!==0||new Set(ranges.map(q=>q.query_id)).size!==ranges.length||ranges.reduce((n,q)=>n+q.expected_rows,0)!==rows||ranges.some((q,i)=>q.expected_rows!==q.read_rows||(i&&q.min<=ranges[i-1].max)))throw Error(`Incomplete load ${name}`);
  const proof={queries:ranges.length,validated_physical_rows:rows,logical_cells:24885,logical_cell_lead_rows:3508785,forecast_float32_values:638598870,overlap:false,ranges};
  await Bun.write(`${b}/${root}/range-summary.json`,JSON.stringify(proof,null,2)+'\n');
  const codebook=Number(metadata.storage?.dictionary?.encoded_bytes??0);
  runs.push({...run,process,load,validation,parts,cache,codebook_bytes:codebook,database_and_codebook_bytes:Number(parts.bytes_on_disk)+codebook,range_proof:proof});
}
const variants=[];
for(const variant of ['raw','profile','packed']){
  const entries=runs.filter(r=>r.variant===variant).sort((a,c)=>a.repeat-c.repeat);
  if(entries.length!==3)throw Error('Missing repeat');
  const times=entries.map(e=>e.process.elapsed_seconds),sorted=[...times].sort((a,c)=>a-c);
  const last=entries[2];
  if(entries.some(e=>Number(e.parts.bytes_on_disk)!==Number(last.parts.bytes_on_disk)))throw Error('Storage changed');
  const root=`runs/${variant}-r3`;
  const exact=await read(`${root}/exact.json`),http=await read(`${root}/http.json`),queries=await read(`${root}/queries.json`),audit=await read(`${root}/independent-audit.json`);
  if(!exact.success||!http.success||http.profiles.length!==27||Object.values(exact.models).some(m=>m.complete_profiles_exact!==100||m.max_coordinate_error_degrees!==0)||queries.models.gfs.count!==300||queries.warmup_queries_per_model!==300)throw Error(`Verification ${variant}`);
  variants.push({variant,workers:4,seconds_by_repeat:times,median_seconds:sorted[1],min_seconds:sorted[0],max_seconds:sorted[2],client_max_rss_kib:Math.max(...entries.map(e=>e.process.max_rss_kib)),database_bytes:Number(last.parts.bytes_on_disk),codebook_bytes:last.codebook_bytes,database_and_codebook_bytes:last.database_and_codebook_bytes,physical_rows:Number(last.parts.storage_rows),logical_cells:24885,logical_rows:3508785,queries:queries.models.gfs,warmup_queries:queries.warmup_queries_per_model,independent_audit:audit,exact:exact.models.gfs,http:{success:http.success,compared_profiles:http.profiles.length,invalid_inputs:http.invalid_inputs,outside_region:http.outside_region,gzip_bytes:http.gzip_bytes}});
}
const source=(await read('source.json')).metadata;
const stage={};
for(const name of ['raw','dictionary']){
  let bytes=0;
  for(const file of (await readdir(`${b}/${name}`)).filter(n=>n.endsWith('.stage.zst')))bytes+=(await stat(`${b}/${name}/${file}`)).size;
  stage[name]={weather_stage_bytes:bytes,preparation:await read(`${name}/preparation.json`)};
}
const summary={time:Date.now()/1000,scope:'Complete GFS staged loads into nine fresh tables, four workers, three repeats per layout, all standard ingestion QC inside the process clock. Acquisition/preparation/full-roundtrip audits outside loader timings. Warm complete-profile loopback queries measured separately.',source:{model:'gfs',dataset:source.dataset,init_time:source.init_time,snapshot_id:source.snapshot_id,cells:source.cells,leads:source.leads,levels:source.levels,variables:source.variables,surface_variables:source.surface_variables,float32_values:638598870},dictionary_preparation:await read('dictionary-prepare-process.json'),raw_preparation:await read('raw-prepare-process.json'),cardinalities:await read('dictionary-cardinalities.json'),cardinality_comparison:await read('cardinality-comparison.json'),full_roundtrip:await read('roundtrip-comparison.json'),legacy_hrrr_codebook:await read('legacy-hrrr-codebook.json'),stage,variants,runs};
await Bun.write(`${b}/summary.json`,JSON.stringify(summary,null,2)+'\n');
console.log(JSON.stringify(variants.map(({variant,seconds_by_repeat,median_seconds,database_and_codebook_bytes,queries})=>({variant,seconds_by_repeat,median_seconds,database_and_codebook_bytes,p50_ms:queries.p50_ms,p95_ms:queries.p95_ms})),null,2));
