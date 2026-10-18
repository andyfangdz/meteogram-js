import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const dir = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(dir, "../../..");
const read = (name) => fs.readFileSync(path.join(dir, name), "utf8");
const json = (name) => JSON.parse(read(name));
const jsonl = (name) => read(name).trim().split("\n").filter(Boolean).map(JSON.parse);
for (const name of fs.readdirSync(dir)) {
  if (name.endsWith(".json")) json(name);
  if (name.endsWith(".jsonl")) jsonl(name);
}
const m = json("metadata.json");
assert.equal(m.cells, 1638757);
assert.equal(m.leads.length, 49);
assert.equal(m.levels.length, 30);
assert.equal(m.variables.length, 6);
assert.equal(m.surface_variables.length, 2);
assert.equal(m.snapshot_id, "WX5GN6QXSDKTBPQQP96G");
assert.equal(m.storage.tile_size, 256);
assert.equal(m.storage.words_per_slot, 32);
assert.equal(m.storage.dictionary.entries, 20164579);
const slots = m.leads.length * (m.levels.length * m.variables.length + 2);
const rows = Math.ceil(m.cells / m.storage.tile_size);
for (const tag of ["gated25601", "gated25602"]) {
  const process = json(`${tag}-process.json`);
  assert.equal(process.exit_code, 0);
  assert(process.elapsed_seconds < 60);
  const load = json(`${tag}-load.json`);
  assert.equal(load.finite_validation_cells, m.cells);
  assert.equal(load.finite_validation_rows, rows);
  assert.equal(load.finite_validation_batches, 101);
  assert.equal(load.logical_rows, m.cells * m.leads.length);
  assert.equal(load.max_active_inserts, 4);
  assert.equal(load.readers, 2);
  assert.equal(load.workers, 6);
  assert.equal(load.candidate_merges_resumed, true);
  const validation = json(`${tag}-validation.json`);
  assert.equal(validation.full_values_checked, true);
  assert.equal(validation.staged_comparison, true);
  assert.equal(validation.samples, 10);
  assert.equal(validation.staged_sample_origin, "immutable_ingestion_buffers");
  assert.equal(validation.logical_weather_values, m.cells * slots);
  assert.equal(validation.stored_indices_checked, rows * m.storage.tile_size * slots);
  const range = json(`${tag}-range-summary.json`);
  assert.equal(range.queries, 101);
  assert.equal(range.read_rows, rows);
  assert.equal(range.read_bytes, 29232146592);
  for (const key of ["all_range_rows_match", "ranges_contiguous_disjoint", "all_physical_rows_covered"]) assert.equal(range[key], true);
  assert.equal(new Set(jsonl(`${tag}-range-queries.jsonl`).map((r) => r.query_id)).size, 101);
  assert.equal(json(`${tag}-server-before.json`).memory_errors, json(`${tag}-server-after.json`).memory_errors);
  assert(jsonl(`${tag}-server-memory.jsonl`).every((r) => r.active_inserts <= 4));
  const exact = json(`${tag}-exact.json`);
  assert.equal(exact.success, true);
  for (const model of Object.values(exact.models)) {
    assert.equal(model.complete_profiles_exact, 100);
    assert.equal(model.max_coordinate_error_degrees, 0);
  }
  const http = json(`${tag}-http.json`);
  assert.equal(http.success, true);
  assert.equal(http.profiles.length, 27);
  assert(http.profiles.every((p) => p.compared));
  assert.equal(http.invalid_inputs, 4);
  assert.equal(http.outside_region, 3);
  assert(http.gzip_bytes > 0);
}
const audit = json("final-gated-independent-audit.json");
assert.equal(audit.finite_validation_mode, "whole_table_scan");
assert.equal(audit.full_values_checked, true);
assert.equal(audit.staged_sample_origin, "staging_files");
assert.equal(audit.samples, 10);
assert.equal(json("final-gated-independent-audit-process.json").exit_code, 0);
assert.equal(jsonl("final-gated-independent-audit-query.jsonl")[0].read_rows, rows);
assert.equal(jsonl("final-gated-independent-audit-query.jsonl")[0].read_bytes, 29232146592);
assert.equal(json("main-preservation.json").all_profile_hashes_identical, true);
assert.equal(json("main-preservation.json").catalog_unchanged, true);
assert.equal(json("main-preservation.json").schedule_unchanged, true);
assert.equal(json("installed-verification.json").success, true);
assert.equal(read("candidate-15.sha256").split(/\s/)[0], read("installed-final.sha256").split(/\s/)[0]);
const canonical = (rows) => rows.map((r) => ({ ...r, rows: Number(r.rows) }));
assert.deepEqual(canonical(jsonl("protected-rows-before.jsonl")), canonical(jsonl("protected-rows-after.jsonl")));
assert.equal(jsonl("protected-rows-after.jsonl").length, 12);
assert.equal(jsonl("remaining-experiment-tables.jsonl").length, 0);
assert.equal(json("staging-cleanup.json").weather_bytes, 50727764653);
assert.equal(read("source-stage-hash-verification.log").trim().split("\n").length, 199);
assert(read("source-stage-hash-verification.log").trim().split("\n").every((line) => line.endsWith(": OK")));
assert.match(read("tests-gated256-final.log"), /39 passed; 0 failed; 0 ignored/);
for (const line of read("source.sha256").trim().split("\n")) {
  const [expected, relative] = line.split(/\s+/, 2);
  assert.equal(createHash("sha256").update(fs.readFileSync(path.join(repo, relative))).digest("hex"), expected);
}
let links = 0;
for (const relative of ["AGENTS.md", "native/README.md", "docs/plans/2026-10-18-hrrr-one-minute-load.md", "docs/benchmarks/2026-10-18-hrrr-one-minute/README.md"]) {
  const filename = path.join(repo, relative);
  for (const match of fs.readFileSync(filename, "utf8").matchAll(/\[[^\]]*\]\(([^)\s]+)\)/g)) {
    const target = match[1].split("#")[0];
    if (!target || /^https?:/.test(target)) continue;
    assert(fs.existsSync(path.resolve(path.dirname(filename), target)), `${relative}: missing ${target}`);
    links++;
  }
}
const result = {
  time: Date.now() / 1000,
  full_scope_verified: true,
  two_external_cli_loads_under_60_seconds: true,
  every_stored_index_and_palette_value_checked: true,
  independent_whole_table_audit_passed: true,
  exact_profiles_and_http_passed: true,
  no_new_memory_errors_during_final_loads: true,
  installed_binary_identical: true,
  protected_tables_and_catalog_preserved: true,
  source_hashes_verified_before_cleanup: true,
  rust_source_hashes_valid: true,
  evidence_json_valid: true,
  local_links_valid: links,
};
fs.writeFileSync(path.join(dir, "completion-audit.json"), `${JSON.stringify(result, null, 2)}\n`);
console.log(JSON.stringify(result));
