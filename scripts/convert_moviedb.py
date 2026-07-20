#!/usr/bin/env python3

import argparse
import datetime
import json
from pathlib import Path
from typing import Iterator, Optional

MAX_TEXT_BYTES = 8_000


def iter_json_array(path: Path) -> Iterator[dict]:
    decoder = json.JSONDecoder()
    with path.open("r", encoding="utf-8") as source:
        buffer = ""
        position = 0
        started = False
        eof = False
        while True:
            if position >= len(buffer) and not eof:
                buffer = source.read(1024 * 1024)
                position = 0
                eof = not buffer
            while position < len(buffer) and buffer[position].isspace():
                position += 1
            if not started:
                if position >= len(buffer):
                    if eof:
                        raise ValueError("MovieDB file is empty")
                    continue
                if buffer[position] != "[":
                    raise ValueError("MovieDB root must be a JSON array")
                position += 1
                started = True
            while position < len(buffer) and (
                buffer[position].isspace() or buffer[position] == ","
            ):
                position += 1
            if position < len(buffer) and buffer[position] == "]":
                return
            try:
                value, end = decoder.raw_decode(buffer, position)
                position = end
                yield value
            except json.JSONDecodeError:
                if eof:
                    raise
                chunk = source.read(1024 * 1024)
                buffer = buffer[position:] + chunk
                position = 0
                eof = not chunk


def timestamp_ms(release_date: str) -> int:
    try:
        value = datetime.date.fromisoformat(release_date)
    except (TypeError, ValueError):
        return 0
    epoch = datetime.date(1970, 1, 1)
    return max(0, (value - epoch).days * 86_400_000)


def truncate_utf8(value: str, maximum: int) -> str:
    encoded = value.encode("utf-8")
    if len(encoded) <= maximum:
        return value
    return encoded[:maximum].decode("utf-8", errors="ignore")


def movie_bucket(movie_id: object, bucket: str, bucket_count: int) -> str:
    if bucket_count < 1:
        raise ValueError("bucket count must be greater than zero")
    if bucket_count == 1:
        return bucket
    try:
        shard = int(movie_id) % bucket_count
    except (TypeError, ValueError):
        encoded = str(movie_id).encode("utf-8")
        shard = int.from_bytes(encoded, byteorder="little") % bucket_count
    return f"{bucket}:{shard:04d}"


def convert_movie(
    movie: dict, bucket: str = "default", bucket_count: int = 1
) -> Optional[dict]:
    movie_id = movie.get("id")
    title = movie.get("title")
    if movie_id is None or not isinstance(title, str) or not title.strip():
        return None
    genres = [
        genre["name"]
        for genre in movie.get("genres", [])
        if isinstance(genre, dict) and isinstance(genre.get("name"), str)
    ]
    text = " ".join(
        value.strip()
        for value in [
            title,
            movie.get("original_title"),
            movie.get("tagline"),
            movie.get("overview"),
            " ".join(genres),
        ]
        if isinstance(value, str) and value.strip()
    )
    release_date = movie.get("release_date") or ""
    return {
        "bucket": movie_bucket(movie_id, bucket, bucket_count),
        "oid": f"movie:{movie_id}",
        "timestamp_ms": timestamp_ms(release_date),
        "text": truncate_utf8(text, MAX_TEXT_BYTES),
        "metadata": {"id": movie_id},
    }


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Convert MovieDB-JSON into generic Sonic NDJSON documents."
    )
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--bucket", default="default")
    parser.add_argument("--bucket-count", type=int, default=1)
    parser.add_argument("--limit", type=int, default=0)
    args = parser.parse_args()
    if args.bucket_count < 1:
        parser.error("--bucket-count must be greater than zero")

    converted = 0
    skipped = 0
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8") as output:
        for movie in iter_json_array(args.input):
            if args.limit and converted + skipped >= args.limit:
                break
            document = convert_movie(movie, args.bucket, args.bucket_count)
            if document is None:
                skipped += 1
                continue
            output.write(
                json.dumps(document, ensure_ascii=False, separators=(",", ":")) + "\n"
            )
            converted += 1
            if converted % 10_000 == 0:
                print(f"Converted {converted} movies")

    print(f"Done: {converted} converted, {skipped} skipped -> {args.output}")


if __name__ == "__main__":
    main()
