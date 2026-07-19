#!/usr/bin/env python3
"""Download Wikipedia language subsets from the Hugging Face `wikimedia/wikipedia`
dataset and convert them to Sonic NDJSON bulk documents, one bucket per language
(or several, when --bucket-count > 1), for sharding/import benchmarks.

Parquet files are downloaded concurrently (--download-workers) and each file is
converted independently into its own cached NDJSON fragment under --cache-dir.
Re-running with the same arguments after an interruption skips every fragment
that already completed, and only re-downloads/re-converts the file that was in
flight when the process stopped.
"""

import argparse
import json
import subprocess
import sys
import time
import zlib
from concurrent.futures import Future, ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator

import pyarrow.parquet as pq
import requests

DATASET = "wikimedia/wikipedia"
MAX_TEXT_BYTES = 8_000
PARQUET_LIST_URL = "https://huggingface.co/api/datasets/{dataset}/parquet/{config}/train"


def list_parquet_files(config: str) -> list[str]:
    response = requests.get(
        PARQUET_LIST_URL.format(dataset=DATASET, config=config), timeout=30
    )
    response.raise_for_status()
    return response.json()


def human_bytes(value: float) -> str:
    for unit in ("B", "KiB", "MiB", "GiB"):
        if value < 1024 or unit == "GiB":
            return f"{value:.1f} {unit}"
        value /= 1024
    return f"{value:.1f} GiB"


def download_file(url: str, destination: Path, label: str) -> Path:
    if destination.is_file():
        print(f"[{label}] using cached {destination}", flush=True)
        return destination
    destination.parent.mkdir(parents=True, exist_ok=True)
    tmp = destination.with_suffix(destination.suffix + ".part")
    started = time.monotonic()
    last_report = started
    downloaded = 0
    with requests.get(url, stream=True, timeout=60) as response:
        response.raise_for_status()
        total = int(response.headers.get("content-length", 0))
        with tmp.open("wb") as handle:
            for chunk in response.iter_content(chunk_size=1024 * 1024):
                handle.write(chunk)
                downloaded += len(chunk)
                now = time.monotonic()
                if now - last_report >= 1.0:
                    last_report = now
                    elapsed = now - started
                    speed = downloaded / elapsed if elapsed else 0
                    if total:
                        percent = 100 * downloaded / total
                        remaining = (total - downloaded) / speed if speed else 0
                        print(
                            f"[{label}] downloading {human_bytes(downloaded)}/{human_bytes(total)} "
                            f"({percent:.1f}%) at {human_bytes(speed)}/s, ETA {remaining:.0f}s",
                            flush=True,
                        )
                    else:
                        print(
                            f"[{label}] downloading {human_bytes(downloaded)} at {human_bytes(speed)}/s",
                            flush=True,
                        )
    tmp.rename(destination)
    elapsed = time.monotonic() - started
    print(
        f"[{label}] downloaded {human_bytes(downloaded)} in {elapsed:.1f}s -> {destination}",
        flush=True,
    )
    return destination


def truncate_utf8(value: str, maximum: int) -> str:
    encoded = value.encode("utf-8")
    if len(encoded) <= maximum:
        return value
    return encoded[:maximum].decode("utf-8", errors="ignore")


def bucket_for(lang: str, key: str, bucket_count: int) -> str:
    if bucket_count <= 1:
        return f"lang:{lang}"
    # CRC32 (not a raw byte/int reinterpretation): sharding keys here share a long common \
    #   prefix (eg. "wiki:fr:1", "wiki:fr:2", ...), and a naive int.from_bytes(...) % N with N \
    #   a power of two would only ever look at that shared prefix's low-order bits, mapping \
    #   every key to the same bucket regardless of its actual suffix.
    shard = zlib.crc32(str(key).encode("utf-8")) % bucket_count
    return f"lang:{lang}:{shard:04d}"


def iter_rows(parquet_path: Path) -> Iterator[dict]:
    parquet_file = pq.ParquetFile(parquet_path)
    for batch in parquet_file.iter_batches(
        batch_size=2_000, columns=["id", "url", "title", "text"]
    ):
        yield from batch.to_pylist()


@dataclass
class FileTask:
    lang: str
    lang_index: int
    lang_total: int
    file_index: int
    file_total: int
    url: str
    parquet_path: Path
    ndjson_path: Path

    @property
    def label(self) -> str:
        return f"{self.lang} {self.lang_index}/{self.lang_total}, file {self.file_index + 1}/{self.file_total}"


def build_tasks(
    languages: list[str], config_prefix: str, cache_dir: Path
) -> list[FileTask]:
    tasks = []
    for lang_index, lang in enumerate(languages, start=1):
        config = f"{config_prefix}.{lang}"
        urls = list_parquet_files(config)
        if not urls:
            raise RuntimeError(f"no parquet files found for config {config}")
        for file_index, url in enumerate(urls):
            tasks.append(
                FileTask(
                    lang=lang,
                    lang_index=lang_index,
                    lang_total=len(languages),
                    file_index=file_index,
                    file_total=len(urls),
                    url=url,
                    parquet_path=cache_dir / lang / f"{file_index:04d}.parquet",
                    ndjson_path=cache_dir / lang / f"{file_index:04d}.ndjson",
                )
            )
    return tasks


def convert_task(task: FileTask, remaining: int) -> tuple[int, bool]:
    """Converts one already-downloaded parquet file into its cached NDJSON fragment.

    The cached fragment always uses a single bucket per language (--bucket-count is
    deliberately ignored here): actual sharding is applied later, at assembly time,
    so that changing --bucket-count between runs never requires re-downloading or
    re-parsing the parquet file, only a cheap re-assembly pass.

    Returns (rows_written, completed). `completed` is only true when the whole file
    was converted (ie. `remaining` never ran out); a truncated fragment is left as
    `.part` so a later run (eg. without --limit-per-language) reconverts it fully
    instead of treating it as done.
    """
    total_rows_in_file = pq.ParquetFile(task.parquet_path).metadata.num_rows
    tmp = task.ndjson_path.with_suffix(task.ndjson_path.suffix + ".part")
    started = time.monotonic()
    last_report = started
    written = 0
    file_seen = 0
    with tmp.open("w", encoding="utf-8") as handle:
        for row in iter_rows(task.parquet_path):
            file_seen += 1
            if remaining <= 0:
                break
            title = (row.get("title") or "").strip()
            text = (row.get("text") or "").strip()
            if title or text:
                document = {
                    "bucket": bucket_for(task.lang, row["id"], 1),
                    "oid": f"wiki:{task.lang}:{row['id']}",
                    "timestamp_ms": 0,
                    "text": truncate_utf8(f"{title} {text}".strip(), MAX_TEXT_BYTES),
                    "metadata": {"title": title, "url": row.get("url"), "lang": task.lang},
                }
                handle.write(json.dumps(document, ensure_ascii=False, separators=(",", ":")))
                handle.write("\n")
                written += 1
                remaining -= 1
            now = time.monotonic()
            if now - last_report >= 2.0:
                last_report = now
                rate = file_seen / (now - started) if now > started else 0
                percent = 100 * file_seen / total_rows_in_file if total_rows_in_file else 0
                print(
                    f"[{task.label}] converted {file_seen}/{total_rows_in_file} rows "
                    f"({percent:.1f}%), {rate:.0f} rows/s",
                    flush=True,
                )

    completed = file_seen >= total_rows_in_file
    if completed:
        tmp.rename(task.ndjson_path)
    print(
        f"[{task.label}] {'done' if completed else 'stopped (limit reached)'}: "
        f"{written} documents written in {time.monotonic() - started:.1f}s",
        flush=True,
    )
    return written, completed


def count_lines(path: Path) -> int:
    # Binary chunked counting: avoids UTF-8 decoding overhead of line-by-line text-mode \
    #   iteration, which matters here since cached fragments can span millions of lines.
    total = 0
    with path.open("rb") as handle:
        while chunk := handle.read(4 * 1024 * 1024):
            total += chunk.count(b"\n")
    return total


def reshard_line(line: str, bucket_count: int) -> tuple[str, str]:
    """Re-derives `bucket` for a cached (always single-bucket-per-language) line \
    according to the current --bucket-count, without touching the cache itself.
    Returns (bucket, rewritten_line)."""
    document = json.loads(line)
    lang = document["metadata"]["lang"]
    bucket = bucket_for(lang, document["oid"], bucket_count)
    document["bucket"] = bucket
    return bucket, json.dumps(document, ensure_ascii=False, separators=(",", ":")) + "\n"


def extract_bucket(line: str) -> str:
    """Reads `bucket` off a cached fragment line without a full JSON parse: every \
    fragment is written with `bucket` as its very first field (see convert_task)."""
    prefix = '{"bucket":"'
    end = line.index('"', len(prefix))
    return line[len(prefix) : end]


def assemble_output(tasks: list[FileTask], output: Path, bucket_count: int) -> int:
    """Concatenates every fragment produced so far, re-sharding each line into
    --bucket-count buckets per language on the fly, then sorts the result by bucket.

    Sorting matters a lot once --bucket-count is high: with many buckets, the handful
    of documents belonging to the same bucket are otherwise scattered essentially
    randomly across the whole file (their position only depends on the source
    parquet's row order, not on their hashed bucket). Sonic's "fresh" import path
    only skips its extra existing-OID check for the *first* time a bucket is written
    to in a given batch; if that bucket's documents are spread across many separate
    batches instead of arriving together, only the very first one is fast and every
    later one pays for an extra read, which is why an unsorted import can start fast
    and then keep slowing down as more and more buckets flip from "brand new" to
    "already seen". Sorting first means every bucket's documents land together in
    (usually) a single batch, so the fast path applies almost everywhere.

    Uses the system `sort` (external merge sort) rather than sorting in memory,
    since this file can be tens of millions of lines.

    Prefers the completed `.ndjson` fragment, but falls back to a truncated
    `.ndjson.part` (eg. a file cut short by --limit-per-language in this very run)
    so its rows are not silently dropped from the final file.
    """
    unsorted_path = output.with_suffix(output.suffix + ".unsorted")
    total = 0
    with unsorted_path.open("w", encoding="utf-8") as destination:
        for task in tasks:
            source = task.ndjson_path
            if not source.is_file():
                part = source.with_suffix(source.suffix + ".part")
                source = part if part.is_file() else None
            if source is None:
                continue
            with source.open("r", encoding="utf-8") as fragment:
                for line in fragment:
                    if bucket_count > 1:
                        bucket, line = reshard_line(line, bucket_count)
                    else:
                        bucket = extract_bucket(line)
                    destination.write(bucket)
                    destination.write("\t")
                    destination.write(line)
                    total += 1

    print(f"Sorting {total} document(s) by bucket...", flush=True)
    started = time.monotonic()
    subprocess.run(
        ["sort", "-t", "\t", "-k1,1", "-s", "-o", str(output), str(unsorted_path)],
        check=True,
    )
    unsorted_path.unlink()
    print(f"Sorted in {time.monotonic() - started:.1f}s", flush=True)

    # Strip the "<bucket>\t" sort-key prefix back out in place.
    stripped_path = output.with_suffix(output.suffix + ".stripped")
    with output.open("r", encoding="utf-8") as source, stripped_path.open(
        "w", encoding="utf-8"
    ) as destination:
        for line in source:
            destination.write(line.split("\t", 1)[1])
    stripped_path.replace(output)

    return total


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--languages",
        required=True,
        help="Comma-separated Wikipedia language codes (eg. en,fr,simple,de)",
    )
    parser.add_argument(
        "--config-prefix",
        default="20231101",
        help="Dataset dump date prefix (default: 20231101)",
    )
    parser.add_argument(
        "--limit-per-language",
        type=int,
        default=0,
        help="Maximum rows to convert per language (default: 0, ie. no limit)",
    )
    parser.add_argument(
        "--bucket-count",
        type=int,
        default=1,
        help="Number of buckets to shard each language into (default: 1, ie. one bucket per language)",
    )
    parser.add_argument(
        "--download-workers",
        type=int,
        default=4,
        help="Number of parquet files to download concurrently (default: 4)",
    )
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=Path(__file__).resolve().parent / "data" / "wikipedia-cache",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.bucket_count < 1:
        sys.exit("--bucket-count must be greater than zero")
    if args.download_workers < 1:
        sys.exit("--download-workers must be greater than zero")
    languages = [language.strip() for language in args.languages.split(",") if language.strip()]
    if not languages:
        sys.exit("--languages must list at least one language code")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()

    print("Listing parquet files...", flush=True)
    tasks = build_tasks(languages, args.config_prefix, args.cache_dir)
    print(f"{len(tasks)} parquet file(s) across {len(languages)} language(s)", flush=True)

    remaining_by_lang = {
        lang: (args.limit_per_language if args.limit_per_language > 0 else float("inf"))
        for lang in languages
    }
    cached_tasks = [task for task in tasks if task.ndjson_path.is_file()]
    pending = [task for task in tasks if not task.ndjson_path.is_file()]
    if cached_tasks:
        print(f"Scanning {len(cached_tasks)} cached fragment(s)...", flush=True)
    scan_started = time.monotonic()
    total_written = 0
    # Fragments already fully converted by a previous, interrupted run: their row count
    # still counts against the per-language limit, but neither the download nor the
    # conversion is redone. Each fragment is only scanned once (not once for the
    # per-language budget and again for the running total).
    for index, task in enumerate(cached_tasks, start=1):
        lines = count_lines(task.ndjson_path)
        remaining_by_lang[task.lang] -= lines
        total_written += lines
        if time.monotonic() - scan_started >= 2.0:
            scan_started = time.monotonic()
            print(
                f"Scanned {index}/{len(cached_tasks)} cached fragment(s), "
                f"{total_written} document(s) so far...",
                flush=True,
            )
    if total_written:
        print(f"Resuming: {total_written} document(s) already cached from a previous run", flush=True)

    with ThreadPoolExecutor(max_workers=args.download_workers) as pool:
        futures: dict[Future[Path], FileTask] = {}
        for task in pending:
            future = pool.submit(download_file, task.url, task.parquet_path, task.label)
            futures[future] = task

        for future in as_completed(futures):
            task = futures[future]
            future.result()
            remaining = remaining_by_lang[task.lang]
            if remaining <= 0:
                print(f"[{task.label}] skipped: per-language limit already reached", flush=True)
                continue
            written, _completed = convert_task(
                task, int(remaining) if remaining != float("inf") else 1 << 62
            )
            remaining_by_lang[task.lang] -= written
            total_written += written
            elapsed_so_far = time.monotonic() - started
            print(
                f"=== Progress: {total_written} document(s) written so far, "
                f"{elapsed_so_far:.1f}s elapsed ===",
                flush=True,
            )

    print(f"Assembling {args.output} (bucket-count={args.bucket_count})...", flush=True)
    assembled = assemble_output(tasks, args.output, args.bucket_count)

    elapsed = time.monotonic() - started
    print(
        f"Wrote {assembled} documents across {len(languages)} language(s) "
        f"to {args.output} in {elapsed:.1f}s"
    )


if __name__ == "__main__":
    main()
