# Full HRRR load under one minute

See the [report](../../plans/2026-10-18-hrrr-one-minute-load.md) for the
measurement boundary, complete dataset, format and deployment details.

The final frozen executable is identified by `candidate-15.sha256`.
`gated25601-*` and `gated25602-*` are the final fresh-table runs with default
settings. External `*-process.json` elapsed time is the acceptance measure;
overlapping work times in `*-load.json` must not be added as wall time.

`*-range-queries.jsonl` records completed ClickHouse checks of all physical
tile ranges. `range-summary.jq` checks complete, disjoint coverage using the
retained metadata. All finite palette values and all stored indices were
validated inside each load. `final-gated-independent-audit-*` records a separate
whole-table scan and rereading staged source samples.

`*-exact.json` compares every coordinate and 100 complete profiles per model
with the immutable source catalog. `*-http.json` compares 27 complete HTTP
forecasts and checks malformed/outside requests and gzip. Query benchmarks
are loopback, warmed filesystem-cache observations, not WAN latency.

Earlier `baseline01`, `tiles6401`, `batch1601`, `batch1602`, `allocator01` and `bounded01` files are
diagnostic history, not the installed binary's acceptance results.
`batch1602` passed all 101 client-side database checks but lost three
completion-log records when background system logging hit the server's RSS
cap. Its incomplete range summary is deliberately retained. The allocator
release change by itself was insufficient: `allocator01` aborted cleanly when
its 16,384-cell batches reached the same cap. The final change also reduces
concurrent inserts and shares SQL bounds constants, without increasing memory
limits. Smaller `bounded01` batches passed with complete records but took
62.41 seconds. The final 256-cell layout and four active-insert permits use
16,384-cell batches. Final runs retain before/after error counts and sampled RSS.

Source preparation and the single-lead live acquisition smoke test are
reported separately. Large weather arrays, codebooks and executables remain
outside Git. Weather staging was removed after its checksums were verified;
metadata, coordinate indexes and codebooks remain on the host. Current/previous/retired
serving tables and historical benchmark tables were protected during cleanup.
