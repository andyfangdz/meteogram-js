import {readdir,stat,unlink} from 'node:fs/promises';
const base='/home/andy/.local/share/meteogram-other-models';
const cleanup=JSON.parse(await Bun.file(`${base}/cleanup.json`).text());
if(cleanup.remaining_experiment_tables!==0)throw Error('Experiment still active');
let files=0,bytes=0;
for(const dir of ['raw-gfs-failed','dictionary-gfs-failed'])for(const name of await readdir(`${base}/${dir}`)) {
  if(!/^(lead|surface)-[0-9]{3}\.stage\.zst\.tmp$/.test(name))continue;
  const file=`${base}/${dir}/${name}`;
  bytes+=(await stat(file)).size;
  await unlink(file);
  files++;
}
cleanup.failed_staging_temp_files_removed=files;
cleanup.failed_staging_temp_bytes_removed=bytes;
cleanup.time=Date.now()/1000;
await Bun.write(`${base}/cleanup.json`,JSON.stringify(cleanup,null,2)+'\n');
console.log(JSON.stringify({files,bytes}));
