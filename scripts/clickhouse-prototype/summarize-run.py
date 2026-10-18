"""Summarize completed resident measurements and explicit operating scenarios."""

import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = args.root
    if not (root / "complete.json").exists():
        raise ValueError("Resident experiment has not completed")
    api = json.loads((root / "api-verification.json").read_text())
    result = {
        "models": {},
        "assumptions": {
            "runs_per_day": 4,
            "days_per_month": 30,
            "reserve_bytes": 20 * 2**30,
            "two_retained_plus_new_build_cycles": 3,
            "merge_allowance_cycles": 1,
            "electricity_is_scenario_not_metered": True,
        },
    }
    for model in ("hrrr", "gfs", "aifs"):
        meta = json.loads((root / model / "metadata.json").read_text())
        source = [
            json.loads((root / model / f"lead-{lead:03d}.json").read_text())
            for lead in meta["leads"]
        ]
        bench = json.loads((root / f"{model}-benchmark.json").read_text())
        staging = json.loads((root / f"{model}-staging.json").read_text())
        fields = {}
        for report in source:
            for name, values in report["fields"].items():
                f = fields.setdefault(
                    name, {"finite": 0, "total": 0, "max_float32_error": 0}
                )
                for key in ("finite", "total"):
                    f[key] += values[key]
                f["max_float32_error"] = max(
                    f["max_float32_error"], values["max_float32_error"]
                )
        result["models"][model] = {
            "snapshot_id": meta["snapshot_id"],
            "init_time": meta["init_time"],
            "cells": meta["cells"],
            "times": len(meta["leads"]),
            "levels": len(meta["levels"]),
            "rows": meta["cells"] * len(meta["leads"]),
            "source_open_seconds": meta["open_seconds"],
            **{
                key: sum(s[key] for s in source)
                for key in (
                    "seconds",
                    "cpu_seconds",
                    "grib_bytes",
                    "chunks",
                    "decode_seconds",
                    "network_rx_bytes",
                    "staged_value_bytes",
                    "process_write_bytes",
                    "map_retries",
                )
            },
            "source_peak_rss_bytes": max(s["peak_rss_bytes"] for s in source),
            "source_cache_downloaded_bytes": sum(
                s["source_cache"]["downloaded_bytes"] for s in source
            ),
            "fields": fields,
            "resident_storage": api["final_resident_storage"][model],
            "ingestion": bench["layouts"]["256_zstd"]["ingestion"],
            "queries": bench["layouts"]["256_zstd"]["queries"],
            "resident_queries": api["resident_database_queries"][model],
            "api_clients": api["models"][model]["clients"],
            "staging_compressed_bytes": sum(s["compressed_bytes"] for s in staging),
            "staging_encode_seconds": sum(s["encode_seconds"] for s in staging),
            "staging_decode_seconds": sum(s["decode_seconds"] for s in staging),
        }
    models = list(result["models"].values())
    total = lambda key: sum(m[key] for m in models)
    storage = sum(m["resident_storage"]["bytes_on_disk"] for m in models)
    raw = total("staged_value_bytes")
    compressed = total("staging_compressed_bytes")
    cache = total("source_cache_downloaded_bytes")
    server_ingest_writes = sum(m["ingestion"]["server_write_bytes"] for m in models)
    samples = [
        json.loads(line) for line in (root / "telemetry.jsonl").read_text().splitlines()
    ]
    server_samples = [
        p for s in samples for p in s["processes"] if p["name"] == "clickhouse"
    ]
    attempts = {}
    for sample in samples:
        for process in sample["processes"]:
            if process["name"] == "clickhouse":
                attempts.setdefault(
                    sample.get("server_attempt_started", "initial"), []
                ).append(process["io"]["write_bytes"])
    server_writes = sum(max(values) - min(values) for values in attempts.values())
    source_writes = total("process_write_bytes")
    result["telemetry"] = {
        "sample_count": len(samples),
        "server_attempt_write_deltas": {
            str(key): max(values) - min(values) for key, values in attempts.items()
        },
        "first_unix": samples[0]["unix"],
        "last_unix": samples[-1]["unix"],
        "minimum_free_bytes": min(s["free_bytes"] for s in samples),
        "sampled_disk_growth_bytes": samples[0]["free_bytes"]
        - min(s["free_bytes"] for s in samples),
        "sampled_server_peak_rss_bytes": max(p["rss"] for p in server_samples),
        "sampled_sum_children_peak_rss_bytes": max(
            sum(p["rss"] for p in s["processes"]) for s in samples
        ),
        "minimum_host_available_memory_bytes": min(
            s["host_available_memory"] for s in samples
        ),
        "server_process_write_delta_bytes": server_writes,
        "host_nvme_write_delta_bytes": samples[-1]["disk_write_bytes"]["nvme0n1"]
        - samples[0]["disk_write_bytes"]["nvme0n1"],
        "limitations": "One-second samples. Initial separately launched HRRR extractor is absent from child telemetry; per-lead RSS/CPU/write measurements cover it. Host disk counters include unrelated activity and exclude the initial HRRR interval before telemetry began. Summed child RSS double-counts shared/mapped pages; no NAND-write measurement.",
    }
    parts = [
        json.loads(line)
        for line in (root / "parts-telemetry.jsonl").read_text().splitlines()
    ]
    inactive_peak = max(
        sum(int(t["bytes"]) for t in sample["tables"] if not t["active"])
        for sample in parts
    )
    result["telemetry"]["observed_inactive_parts_peak_bytes"] = inactive_peak
    result["telemetry"]["parts_samples_start_unix"] = parts[0]["unix"]
    result["capacity_scenarios"] = {
        "single_cycle_resident_bytes": storage,
        "raw_staging_bytes": raw,
        "compressed_staging_bytes": compressed,
        "source_payload_cache_bytes": cache,
        "raw_staging_two_retained_new_build_merge_cache_reserve_bytes": 4 * storage
        + raw
        + cache
        + 20 * 2**30,
        "compressed_staging_two_retained_new_build_merge_cache_reserve_bytes": 4
        * storage
        + compressed
        + cache
        + 20 * 2**30,
        "raw_staging_with_observed_obsolete_parts_bytes": 4 * storage
        + inactive_peak
        + raw
        + cache
        + 20 * 2**30,
        "compressed_staging_with_observed_obsolete_parts_bytes": 4 * storage
        + inactive_peak
        + compressed
        + cache
        + 20 * 2**30,
        "limitations": "Capacity arithmetic, not a measured second-cycle publication/retention test. The conservative variants additionally budget the observed inactive-part peak separately from a full-cycle merge-output allowance. Includes one new source cache, not old caches; excludes OS, environment, logs and source metadata. Compressed staging is measured codec output, not the implemented writer.",
    }
    result["monthly_scenarios"] = {
        "successful_source_payload_bytes": cache * 120,
        "source_payload_plus_10_percent_bytes": cache * 132,
        "source_payload_plus_20_percent_bytes": cache * 144,
        "raw_staging_logical_bytes": raw * 120,
        "source_process_write_bytes": source_writes * 120,
        "server_ingestion_process_write_bytes": server_ingest_writes * 120,
        "server_whole_experiment_process_write_bytes": server_writes * 120,
        "source_plus_server_logical_write_bytes": (source_writes + server_writes) * 120,
        "electricity": [
            {
                "continuous_incremental_watts": w,
                "kwh_per_month": w * 24 * 30 / 1000,
                "usd_at_0_15_per_kwh": w * 24 * 30 / 1000 * 0.15,
                "usd_at_0_25_per_kwh": w * 24 * 30 / 1000 * 0.25,
                "usd_at_0_30_per_kwh": w * 24 * 30 / 1000 * 0.30,
            }
            for w in (5, 10, 20, 30, 40, 80)
        ],
        "limitations": "120 comparable complete cycles; source bytes exclude metadata/protocol/failed transfers. 10%/20% are allowances, not measured retry overhead. Process writes are logical Linux counters, not SSD endurance. Power and tariffs are scenarios, not measurements or quoted prices.",
    }
    result["timing"] = {
        "source_fetch_decode_stage_seconds": total("seconds"),
        "source_cpu_seconds": total("cpu_seconds"),
        "load_seconds": sum(m["ingestion"]["seconds"] for m in models),
        "fetch_plus_load_seconds": total("seconds")
        + sum(m["ingestion"]["seconds"] for m in models),
        "experiment": json.loads((root / "complete.json").read_text()),
        "limitations": "Source timers overlap download/decode/gather/write and exclude dataset-open time. Summed decode wall time is not CPU time. Experiment includes validation, codec tests and API load tests; neither timing proves production update availability.",
    }
    initial = root / "initial-acquisition.json"
    if initial.exists():
        initial = json.loads(initial.read_text())
        result["timing"]["from_initial_acquisition_log_creation_seconds"] = (
            result["timing"]["experiment"]["completed_unix"]
            - initial["source_command_started_unix"]
        )
        result["timing"]["initial_hrrr_open_seconds"] = initial["initial_selection"][
            "open_seconds"
        ]
    args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
