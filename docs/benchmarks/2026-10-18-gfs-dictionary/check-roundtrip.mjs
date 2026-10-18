import {readdir,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const b='/home/andy/.local/share/meteogram-gfs-dictionary';
const hash=async file=>createHash('sha256').update(await readFile(file)).digest('hex');
const names=(await readdir(`${b}/raw`)).filter(n=>n.endsWith('.stage.zst')||n.endsWith('.stage.json')||n==='cells.bin.zst').sort();
if(names.length!==565)throw Error('Incomplete raw forecast');
const fingerprints={};
for(const n of names)fingerprints[n]=await hash(`${b}/raw/${n}`);
const summary={success:true,compared_layouts:['profile','packed'],files_per_layout:names.length,weather_payloads_per_layout:282,weather_index_files_per_layout:282,coordinate_files_per_layout:1,cells:24885,leads:141,levels:30,pressure_fields:6,surface_fields:2,float32_values_per_layout:638598870,mismatches:0,comparison:'SHA-256 equality of every deterministic Float32 stage payload/index and native coordinate file recovered from each complete loaded dictionary table'};
for(const variant of summary.compared_layouts){
  const lines=[];
  for(const n of names){const actual=await hash(`${b}/roundtrip-${variant}/${n}`);if(actual!==fingerprints[n])throw Error(`Full raw bits differ ${variant} ${n}`);lines.push(`${actual}  ${n}`);}
  await Bun.write(`${b}/runs/${variant}-r3/full-roundtrip.sha256`,lines.join('\n')+'\n');
}
await Bun.write(`${b}/roundtrip-comparison.json`,JSON.stringify(summary,null,2)+'\n');
console.log(JSON.stringify(summary));
