#!/usr/bin/env python3
"""Aggregate Sonic ingest NDJSON profiles across one or more backends."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path
from typing import Iterable


PHASES = {
    "protocol": ("base64_decode_us", "decompress_us", "json_decode_us"),
    "tokenize": ("tokenize_us",),
    "locks": (
        "kv_access_wait_us",
        "fst_access_wait_us",
        "kv_acquire_us",
        "kv_lock_wait_us",
    ),
    "kv_reads": (
        "kv_metadata_us",
        "oid_reads_us",
        "term_reads_us",
        "posting_reads_us",
        "frequency_reads_us",
        "time_posting_reads_us",
    ),
    "kv_cpu": ("document_encode_us", "batch_finalize_us", "kv_cpu_other_us"),
    "rocksdb_write": ("rocksdb_write_us",),
    "fst": ("fst_us",),
}

WRITE_PHASES = {
    "wal": "write_wal_us",
    "memtable": "write_memtable_us",
    "delay": "write_delay_us",
    "pre_post": "write_pre_and_post_us",
    "db_mutex": "write_db_mutex_us",
    "condition_wait": "write_db_condition_wait_us",
    "merge_operator": "write_merge_operator_us",
}


def parse_args() -> argparse.Namespace:
    default = str(Path(__file__).parent / "data/logs/sonic-*.ingest.ndjson")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="*", type=Path, help=f"Profile files (default: {default})")
    parser.add_argument("--window", type=float, default=10.0, help="Window size in seconds")
    return parser.parse_args()


def default_files() -> list[Path]:
    return sorted((Path(__file__).parent / "data/logs").glob("sonic-*.ingest.ndjson"))


def load_records(paths: Iterable[Path]) -> list[dict]:
    records = []
    for path in paths:
        try:
            lines = path.open(encoding="utf-8")
        except OSError as error:
            print(f"Skipping {path}: {error}")
            continue
        with lines:
            for line_number, line in enumerate(lines, 1):
                try:
                    record = json.loads(line)
                    record["_source"] = path.name
                    records.append(record)
                except json.JSONDecodeError as error:
                    print(f"Skipping {path}:{line_number}: {error}")
    return sorted(records, key=lambda record: record["timestamp_ms"])


def percentile(values: list[int], fraction: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    weight = position - lower
    return ordered[lower] * (1 - weight) + ordered[upper] * weight


def phase_totals(records: Iterable[dict]) -> dict[str, int]:
    return {
        phase: sum(sum(record.get(field, 0) for field in fields) for record in records)
        for phase, fields in PHASES.items()
    }


def dominant_phase(records: list[dict]) -> str:
    totals = phase_totals(records)
    return max(totals, key=totals.get) if totals else "unknown"


def print_windows(records: list[dict], window_seconds: float) -> None:
    window_ms = max(1, int(window_seconds * 1000))
    started_ms = records[0]["timestamp_ms"]
    windows: dict[int, list[dict]] = defaultdict(list)
    for record in records:
        windows[(record["timestamp_ms"] - started_ms) // window_ms].append(record)

    print("\nWindows")
    print("elapsed   docs/s  batches  p95 batch  dominant       L0 i/p/d  pending i/p/d")
    for index in sorted(windows):
        window = windows[index]
        documents = sum(record["documents"] for record in window)
        p95_ms = percentile([record["command_total_us"] for record in window], 0.95) / 1000
        index_l0 = max(record.get("l0_index") or 0 for record in window)
        postings_l0 = max(record.get("l0_postings") or 0 for record in window)
        documents_l0 = max(record.get("l0_documents") or 0 for record in window)
        index_pending = max(
            record.get("pending_compaction_index_bytes") or 0 for record in window
        )
        documents_pending = max(
            record.get("pending_compaction_documents_bytes") or 0 for record in window
        )
        postings_pending = max(
            record.get("pending_compaction_postings_bytes") or 0 for record in window
        )
        print(
            f"{index * window_seconds:7.0f}s "
            f"{documents / window_seconds:8.0f} "
            f"{len(window):8d} "
            f"{p95_ms:8.1f}ms "
            f"{dominant_phase(window):13s} "
            f"{index_l0:2d}/{postings_l0:2d}/{documents_l0:<2d} "
            f"{index_pending / (1024 * 1024):5.0f}/"
            f"{postings_pending / (1024 * 1024):.0f}/"
            f"{documents_pending / (1024 * 1024):<5.0f}MB"
        )


def print_summary(records: list[dict], file_count: int) -> None:
    documents = sum(record["documents"] for record in records)
    command_times = [record["command_total_us"] for record in records]
    total_command_us = sum(command_times)
    phases = phase_totals(records)

    print(
        f"Loaded {len(records)} batches and {documents} documents "
        f"from {file_count} backend profile(s)."
    )
    print(
        f"Batch latency: p50={percentile(command_times, 0.50) / 1000:.1f}ms "
        f"p95={percentile(command_times, 0.95) / 1000:.1f}ms "
        f"p99={percentile(command_times, 0.99) / 1000:.1f}ms"
    )
    print("\nPhase totals")
    for phase, elapsed_us in sorted(phases.items(), key=lambda item: item[1], reverse=True):
        percentage = elapsed_us * 100 / total_command_us if total_command_us else 0
        print(f"{phase:14s} {elapsed_us / 1_000_000:10.1f}s {percentage:6.1f}%")

    rocksdb_write_us = sum(record.get("rocksdb_write_us", 0) for record in records)
    print("\nRocksDB write breakdown")
    for phase, field in WRITE_PHASES.items():
        elapsed_us = sum(record.get(field, 0) for record in records)
        percentage = elapsed_us * 100 / rocksdb_write_us if rocksdb_write_us else 0
        print(f"{phase:18s} {elapsed_us / 1_000_000:10.1f}s {percentage:6.1f}%")

    print("\nWriteBatch totals")
    for label, field in (
        ("index", "batch_index_bytes"),
        ("postings", "batch_postings_bytes"),
        ("documents", "batch_documents_bytes"),
    ):
        size = sum(record.get(field, 0) for record in records)
        print(f"{label:18s} {size / (1024 * 1024):10.1f}MB")
    print(
        "operations         "
        f"put={sum(record.get('batch_put_count', 0) for record in records)} "
        f"delete={sum(record.get('batch_delete_count', 0) for record in records)} "
        f"merge={sum(record.get('batch_merge_count', 0) for record in records)}"
    )

    stopped = sum(1 for record in records if record.get("write_stopped"))
    delayed_rate = max((record.get("delayed_write_rate") or 0) for record in records)
    index_pending = max(
        record.get("pending_compaction_index_bytes") or 0 for record in records
    )
    documents_pending = max(
        record.get("pending_compaction_documents_bytes") or 0 for record in records
    )
    postings_pending = max(
        record.get("pending_compaction_postings_bytes") or 0 for record in records
    )
    print(
        f"\nRocksDB: write_stopped_batches={stopped}, "
        f"max_delayed_write_rate={delayed_rate}, "
        f"max_pending_index={index_pending / (1024 * 1024):.0f}MB, "
        f"max_pending_postings={postings_pending / (1024 * 1024):.0f}MB, "
        f"max_pending_documents={documents_pending / (1024 * 1024):.0f}MB"
    )
    print(
        "FST: max_pending_consolidations="
        f"{max(record.get('fst_pending_consolidations', 0) for record in records)}"
    )
    print(f"Largest wall-time phase: {dominant_phase(records)}")


def main() -> None:
    args = parse_args()
    paths = args.files or default_files()
    if not paths:
        raise SystemExit("No ingest profile files found.")
    records = load_records(paths)
    if not records:
        raise SystemExit("No valid ingest profile records found.")
    print_summary(records, len(paths))
    print_windows(records, args.window)


if __name__ == "__main__":
    main()
