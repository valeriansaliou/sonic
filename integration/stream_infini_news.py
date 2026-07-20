#!/usr/bin/env python3
"""Stream Hugging Face Infini-News rows as Sonic NDJSON documents."""

import argparse
import hashlib
import importlib
import json
import os
import sys
import time
from datetime import UTC, datetime
from typing import Any, Iterator


DATASET = "ruggsea/infini-news-corpus"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--year", type=int, default=2026)
    parser.add_argument("--month", type=int, default=4)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--bucket-count", type=int, default=100)
    parser.add_argument("--max-text-bytes", type=int, default=8_000)
    parser.add_argument("--source-batch-rows", type=int, default=1_000)
    parser.add_argument("--progress-every", type=int, default=10_000)
    return parser.parse_args()


def truncate_utf8(value: str, maximum: int) -> str:
    encoded = value.encode("utf-8")
    if len(encoded) <= maximum:
        return value
    return encoded[:maximum].decode("utf-8", errors="ignore")


def timestamp_ms(*values: Any) -> int:
    for value in values:
        if not isinstance(value, str) or not value:
            continue
        try:
            parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
            if parsed.tzinfo is None:
                parsed = parsed.replace(tzinfo=UTC)
            return int(parsed.timestamp() * 1_000)
        except ValueError:
            continue
    return 0


def compact_metadata(row: dict[str, Any]) -> dict[str, Any]:
    metadata = {
        "title": row.get("title"),
        "url": row.get("url"),
        "hostname": row.get("url_hostname"),
        "lang": row.get("lang") or row.get("language_short"),
        "topic": row.get("iptc_topic"),
        "publish_date": row.get("publish_date"),
    }
    return {key: value for key, value in metadata.items() if value not in (None, "")}


def convert_row(
    row: dict[str, Any], row_number: int, bucket_count: int, max_text_bytes: int
) -> dict[str, Any] | None:
    parts = [
        value.strip()
        for value in (row.get("title"), row.get("description"), row.get("text"))
        if isinstance(value, str) and value.strip()
    ]
    if not parts:
        return None

    identity = row.get("warc_record_id")
    if not isinstance(identity, str) or not identity:
        identity = f"{row.get('url', '')}:{row.get('warc_date', '')}:{row_number}"
    digest = hashlib.blake2b(identity.encode("utf-8"), digest_size=16).digest()
    lang = row.get("lang") or row.get("language_short") or "unknown"
    shard = int.from_bytes(digest[:8], "big") % bucket_count

    return {
        "bucket": f"news:{lang}:{shard:04d}",
        "oid": f"news:{digest.hex()}",
        "timestamp_ms": timestamp_ms(row.get("publish_date"), row.get("warc_date")),
        "text": truncate_utf8("\n".join(parts), max_text_bytes),
        "metadata": compact_metadata(row),
    }


def iter_rows(dataset: Any, batch_size: int) -> Iterator[dict[str, Any]]:
    for batch in dataset.iter(batch_size=batch_size):
        row_count = len(next(iter(batch.values()), []))
        for index in range(row_count):
            yield {key: values[index] for key, values in batch.items()}


def load_stream(year: int, month: int) -> Any:
    try:
        load_dataset = importlib.import_module("datasets").load_dataset
    except ImportError:
        sys.exit("Missing dependency: install it with `python -m pip install datasets`")

    data_files = f"data/year={year}/month={month:02d}/part-*.parquet"
    print(f"Streaming {DATASET}/{data_files}", file=sys.stderr)
    dataset = load_dataset(
        DATASET,
        data_files=data_files,
        split="train",
        streaming=True,
        token=os.environ.get("HF_TOKEN"),
    )
    if "text_xxhash64" in (dataset.column_names or []):
        dataset = dataset.remove_columns(["text_xxhash64"])
    return dataset


def main() -> int:
    args = parse_args()
    if not 1 <= args.month <= 12:
        sys.exit("--month must be between 1 and 12")
    if min(args.bucket_count, args.max_text_bytes, args.source_batch_rows) < 1:
        sys.exit("--bucket-count, --max-text-bytes and --source-batch-rows must be positive")

    dataset = load_stream(args.year, args.month)
    started = time.monotonic()
    seen = 0
    written = 0

    try:
        for seen, row in enumerate(iter_rows(dataset, args.source_batch_rows), 1):
            if args.limit and seen > args.limit:
                break
            document = convert_row(row, seen, args.bucket_count, args.max_text_bytes)
            if document is not None:
                sys.stdout.write(
                    json.dumps(document, ensure_ascii=False, separators=(",", ":")) + "\n"
                )
                written += 1
            if args.progress_every and seen % args.progress_every == 0:
                elapsed = time.monotonic() - started
                print(
                    f"Converted {written}/{seen} rows ({written / elapsed:.0f} documents/s)",
                    file=sys.stderr,
                )
    except BrokenPipeError:
        return 0

    elapsed = time.monotonic() - started
    print(
        f"Converted {written}/{seen} rows in {elapsed:.1f}s "
        f"({written / elapsed if elapsed else 0:.0f} documents/s)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
