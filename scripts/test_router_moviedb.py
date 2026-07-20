#!/usr/bin/env python3

import argparse
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import time
from collections import Counter
from pathlib import Path
from typing import Optional


ROOT_DIR = Path(__file__).resolve().parents[1]
DEFAULT_STATE_DIR = ROOT_DIR / ".data" / "test-router-moviedb"
DEFAULT_DATASET_DIR = ROOT_DIR / ".data" / "demo-moviedb" / "MovieDB-JSON"
PASSWORD = "SecretPassword"
ADMIN_PASSWORD = "RouterAdminPassword"
COLLECTION = "movies"
RUST_VERSION = "1.91.0"


def run(
    command: list[str],
    *,
    cwd: Path = ROOT_DIR,
    env: Optional[dict[str, str]] = None,
    capture: bool = False,
) -> subprocess.CompletedProcess[str]:
    print("+", " ".join(str(part) for part in command), flush=True)
    return subprocess.run(
        command,
        cwd=cwd,
        env=env,
        check=True,
        text=True,
        capture_output=capture,
    )


def require_commands(commands: list[str]) -> None:
    missing = [command for command in commands if shutil.which(command) is None]
    if missing:
        raise RuntimeError(f"missing required commands: {', '.join(missing)}")


def cargo_environment() -> tuple[list[str], dict[str, str]]:
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(ROOT_DIR / "target")
    version = subprocess.run(
        ["rustc", "--version"], check=True, text=True, capture_output=True
    ).stdout
    match = re.search(r"rustc (\d+)\.(\d+)\.(\d+)", version)
    if match and tuple(map(int, match.groups())) >= (1, 91, 0):
        return ["cargo"], environment
    if shutil.which("rustup"):
        toolchains = subprocess.run(
            ["rustup", "toolchain", "list"],
            check=True,
            text=True,
            capture_output=True,
        ).stdout
        if any(line.startswith(RUST_VERSION) for line in toolchains.splitlines()):
            environment["RUSTC"] = subprocess.run(
                ["rustup", "which", "--toolchain", RUST_VERSION, "rustc"],
                check=True,
                text=True,
                capture_output=True,
            ).stdout.strip()
            environment["RUSTDOC"] = subprocess.run(
                ["rustup", "which", "--toolchain", RUST_VERSION, "rustdoc"],
                check=True,
                text=True,
                capture_output=True,
            ).stdout.strip()
            return ["rustup", "run", RUST_VERSION, "cargo"], environment
    raise RuntimeError(
        f"Rust {RUST_VERSION} or newer is required; active compiler is {version.strip()}"
    )


def prepare_dataset(dataset_dir: Path, state_dir: Path) -> Path:
    movies_json = dataset_dir / "movies.json"
    if not (dataset_dir / ".git").is_dir():
        dataset_dir.parent.mkdir(parents=True, exist_ok=True)
        run(
            [
                "git",
                "clone",
                "--depth",
                "1",
                "https://github.com/tn3w/MovieDB-JSON.git",
                str(dataset_dir),
            ]
        )
    if not movies_json.is_file():
        merged = state_dir / "movies-merged.zip"
        run(
            ["zip", "-s", "0", "movies.zip", "--out", str(merged)],
            cwd=dataset_dir,
        )
        run(["unzip", "-o", str(merged), "-d", str(dataset_dir)])
    if not movies_json.is_file():
        raise RuntimeError(f"MovieDB archive did not create {movies_json}")
    return movies_json


def build_binaries() -> None:
    cargo, environment = cargo_environment()
    run(
        cargo
        + [
            "build",
            "--locked",
            "--release",
            "--bin",
            "sonic",
            "--no-default-features",
            "-F",
            "stemming",
        ],
        env=environment,
    )
    run(
        cargo
        + [
            "build",
            "--locked",
            "--release",
            "-p",
            "sonic-router",
            "--bin",
            "sonic-router",
        ],
        env=environment,
    )
    run(
        cargo
        + [
            "build",
            "--locked",
            "--release",
            "-p",
            "sonic_client",
            "--bin",
            "sonic-cli",
        ],
        env=environment,
    )


def wait_for_port(
    address: tuple[str, int], process: subprocess.Popen[str], log_path: Path
) -> None:
    for _ in range(200):
        if process.poll() is not None:
            raise RuntimeError(
                f"process exited with code {process.returncode}; see {log_path}"
            )
        try:
            with socket.create_connection(address, timeout=0.1):
                return
        except OSError:
            time.sleep(0.1)
    raise RuntimeError(f"process did not listen on {address}; see {log_path}")


class ProcessGroup:
    def __init__(self, keep_running: bool) -> None:
        self.keep_running = keep_running
        self.processes: list[tuple[str, subprocess.Popen[str], object]] = []
        self.completed = False

    def start(
        self, name: str, command: list[str], log_path: Path, env: dict[str, str]
    ) -> subprocess.Popen[str]:
        log = log_path.open("w", encoding="utf-8")
        process = subprocess.Popen(
            command,
            cwd=ROOT_DIR,
            env=env,
            stdout=log,
            stderr=subprocess.STDOUT,
            text=True,
        )
        self.processes.append((name, process, log))
        return process

    def close(self) -> None:
        if self.keep_running and self.completed:
            for name, process, log in self.processes:
                log.flush()
                print(f"{name} is still running with PID {process.pid}")
            return
        for _, process, _ in reversed(self.processes):
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
        for _, process, log in reversed(self.processes):
            if process.poll() is None:
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            log.close()


def write_router_config(
    path: Path,
    directory_path: Path,
    router_port: int,
    admin_port: int,
    backend_ports: list[int],
) -> None:
    lines = [
        "[server]",
        'log_level = "info"',
        "",
        "[channel]",
        f'inet = "127.0.0.1:{router_port}"',
        "tcp_timeout = 300",
        "bulk_buffer_size = 8388608",
        f'auth_password = "{PASSWORD}"',
        "",
        "[admin]",
        f'inet = "127.0.0.1:{admin_port}"',
        f'auth_password = "{ADMIN_PASSWORD}"',
        "",
        "[directory]",
        f"path = {json.dumps(str(directory_path))}",
        "",
    ]
    for index, port in enumerate(backend_ports):
        lines.extend(
            [
                "[[servers]]",
                f'id = "sonic-{index}"',
                f'address = "127.0.0.1:{port}"',
                f'auth_password = "{PASSWORD}"',
                'status = "active"',
                "weight = 1",
                "",
            ]
        )
    path.write_text("\n".join(lines), encoding="utf-8")


def admin_command(port: int, command: str) -> object:
    with socket.create_connection(("127.0.0.1", port), timeout=10) as stream:
        stream.settimeout(10)
        reader = stream.makefile("r", encoding="utf-8", newline="\n")
        writer = stream.makefile("w", encoding="utf-8", newline="\n")
        writer.write(f"AUTH {ADMIN_PASSWORD}\n")
        writer.flush()
        authenticated = json.loads(reader.readline())
        if not authenticated.get("ok"):
            raise RuntimeError(f"router admin authentication failed: {authenticated}")
        writer.write(f"{command}\n")
        writer.flush()
        response = json.loads(reader.readline())
        if not response.get("ok"):
            raise RuntimeError(f"{command} failed: {response.get('error')}")
        return response.get("data")


def channel_command(port: int, mode: str, command: str) -> str:
    with socket.create_connection(("127.0.0.1", port), timeout=30) as stream:
        stream.settimeout(30)
        reader = stream.makefile("r", encoding="utf-8", newline="\n")
        writer = stream.makefile("w", encoding="utf-8", newline="\n")
        greeting = reader.readline().strip()
        if not greeting.startswith("CONNECTED "):
            raise RuntimeError(f"unexpected Sonic greeting: {greeting}")
        writer.write(f"START {mode} {PASSWORD}\n")
        writer.flush()
        started = reader.readline().strip()
        if not started.startswith("STARTED "):
            raise RuntimeError(f"Sonic channel did not start: {started}")
        writer.write(f"{command}\n")
        writer.flush()
        response = reader.readline().strip()
        if response.startswith("ERR "):
            raise RuntimeError(f"{command} failed: {response}")
        return response


def import_documents(
    cli: Path, port: int, path: Path, batch_documents: int
) -> dict[str, int]:
    result = run(
        [
            str(cli),
            "--addr",
            f"127.0.0.1:{port}",
            "--password",
            PASSWORD,
            "--json",
            "import",
            "--file",
            str(path),
            "--collection",
            COLLECTION,
            "--mode",
            "upsert",
            "--batch-documents",
            str(batch_documents),
        ],
        capture=True,
    )
    return json.loads(result.stdout)


def query_documents(
    cli: Path, port: int, bucket: str, terms: str
) -> list[dict[str, object]]:
    result = run(
        [
            str(cli),
            "--addr",
            f"127.0.0.1:{port}",
            "--password",
            PASSWORD,
            "--json",
            "query",
            "--collection",
            COLLECTION,
            "--bucket",
            bucket,
            "--documents",
            "--limit",
            "100",
            terms,
        ],
        capture=True,
    )
    return json.loads(result.stdout)


def load_bucket_samples(path: Path) -> tuple[dict[str, dict], Counter[str]]:
    samples: dict[str, dict] = {}
    counts: Counter[str] = Counter()
    with path.open("r", encoding="utf-8") as source:
        for line in source:
            document = json.loads(line)
            bucket = document["bucket"]
            counts[bucket] += 1
            samples.setdefault(bucket, document)
    return samples, counts


def query_terms(text: str) -> list[str]:
    words = re.findall(r"[A-Za-z0-9]{4,}", text)
    candidates = []
    for size in (4, 3, 2, 1):
        if len(words) >= size:
            candidates.append(" ".join(words[:size]))
    if words:
        candidates.append(max(words, key=len))
    return list(dict.fromkeys(candidates))


def find_probe(
    cli: Path, port: int, bucket: str, document: dict
) -> tuple[str, list[dict[str, object]]]:
    expected = document["oid"]
    for terms in query_terms(document["text"]):
        results = query_documents(cli, port, bucket, terms)
        if any(result.get("oid") == expected for result in results):
            return terms, results
    raise RuntimeError(f"could not query {expected} in {bucket}")


def validate_placements(
    snapshot: dict, expected_buckets: set[str], backend_count: int
) -> Counter[str]:
    placements = list(snapshot["placements"].values())
    actual_buckets = {placement["bucket"] for placement in placements}
    if actual_buckets != expected_buckets:
        missing = sorted(expected_buckets - actual_buckets)
        extra = sorted(actual_buckets - expected_buckets)
        raise RuntimeError(f"placement mismatch; missing={missing}, extra={extra}")
    distribution = Counter(placement["primary"] for placement in placements)
    if len(distribution) != backend_count:
        raise RuntimeError(f"not all backends received buckets: {distribution}")
    if max(distribution.values()) - min(distribution.values()) > 1:
        raise RuntimeError(f"unbalanced bucket distribution: {distribution}")
    return distribution


def select_migration(
    snapshot: dict, probes: dict[str, tuple[str, dict]]
) -> tuple[str, str, str]:
    backends = sorted(snapshot["backends"])
    for placement in snapshot["placements"].values():
        bucket = placement["bucket"]
        if bucket in probes:
            source = placement["primary"]
            target = next(backend for backend in backends if backend != source)
            return bucket, source, target
    raise RuntimeError("no validated bucket is available for migration")


def write_bucket_file(source: Path, output: Path, bucket: str) -> int:
    written = 0
    with source.open("r", encoding="utf-8") as reader, output.open(
        "w", encoding="utf-8"
    ) as writer:
        for line in reader:
            document = json.loads(line)
            if document["bucket"] == bucket:
                writer.write(line)
                written += 1
    return written


def parse_count(response: str) -> int:
    parts = response.split()
    if len(parts) != 2 or parts[0] != "RESULT":
        raise RuntimeError(f"unexpected COUNT response: {response}")
    return int(parts[1])


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a local MovieDB sharding and migration test."
    )
    parser.add_argument("--limit", type=int, default=10_000)
    parser.add_argument("--bucket-count", type=int, default=64)
    parser.add_argument("--batch-documents", type=int, default=1_000)
    parser.add_argument("--state-dir", type=Path, default=DEFAULT_STATE_DIR)
    parser.add_argument("--dataset-dir", type=Path, default=DEFAULT_DATASET_DIR)
    parser.add_argument("--base-port", type=int, default=15_900)
    parser.add_argument("--keep-running", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    if args.limit < 0:
        parser.error("--limit must be zero or greater")
    if args.bucket_count < 2:
        parser.error("--bucket-count must be at least two")
    if args.batch_documents < 1:
        parser.error("--batch-documents must be greater than zero")
    return args


def main() -> None:
    args = parse_args()
    require_commands(["cargo", "git", "python3", "unzip", "zip"])
    state_dir = args.state_dir.resolve()
    runtime_dir = state_dir / "runtime"
    if runtime_dir.exists():
        shutil.rmtree(runtime_dir)
    runtime_dir.mkdir(parents=True)

    movies_json = prepare_dataset(args.dataset_dir.resolve(), state_dir)
    movies_ndjson = state_dir / f"movies-{args.bucket_count}.ndjson"
    run(
        [
            sys.executable,
            str(ROOT_DIR / "scripts" / "convert_moviedb.py"),
            str(movies_json),
            str(movies_ndjson),
            "--bucket",
            "movie-shard",
            "--bucket-count",
            str(args.bucket_count),
            "--limit",
            str(args.limit),
        ]
    )
    samples, bucket_counts = load_bucket_samples(movies_ndjson)
    if len(samples) < 2:
        raise RuntimeError("the converted dataset must contain at least two buckets")

    if not args.skip_build:
        build_binaries()
    sonic = ROOT_DIR / "target" / "release" / "sonic"
    router = ROOT_DIR / "target" / "release" / "sonic-router"
    cli = ROOT_DIR / "target" / "release" / "sonic-cli"
    for binary in (sonic, router, cli):
        if not binary.is_file():
            raise RuntimeError(f"missing binary: {binary}; remove --skip-build")

    router_port = args.base_port
    admin_port = args.base_port + 1
    backend_ports = [args.base_port + offset for offset in (2, 3, 4)]
    router_config = runtime_dir / "router.cfg"
    write_router_config(
        router_config,
        runtime_dir / "directory.db",
        router_port,
        admin_port,
        backend_ports,
    )

    processes = ProcessGroup(args.keep_running)
    try:
        for index, port in enumerate(backend_ports):
            backend_dir = runtime_dir / f"sonic-{index}"
            environment = {
                **os.environ,
                "SONIC_CHANNEL__INET": f"127.0.0.1:{port}",
                "SONIC_CHANNEL__AUTH_PASSWORD": PASSWORD,
                "SONIC_SERVER__LOG_LEVEL": "error",
                "SONIC_STORE__KV__PATH": str(backend_dir / "kv"),
                "SONIC_STORE__FST__PATH": str(backend_dir / "fst"),
            }
            log_path = runtime_dir / f"sonic-{index}.log"
            process = processes.start(
                f"sonic-{index}",
                [str(sonic), "-c", str(ROOT_DIR / "config.cfg")],
                log_path,
                environment,
            )
            wait_for_port(("127.0.0.1", port), process, log_path)

        router_log = runtime_dir / "router.log"
        router_process = processes.start(
            "sonic-router",
            [str(router), "-c", str(router_config)],
            router_log,
            os.environ.copy(),
        )
        wait_for_port(("127.0.0.1", router_port), router_process, router_log)
        wait_for_port(("127.0.0.1", admin_port), router_process, router_log)

        print(f"Importing {sum(bucket_counts.values())} MovieDB documents...")
        summary = import_documents(cli, router_port, movies_ndjson, args.batch_documents)
        if summary["imported"] != sum(bucket_counts.values()) or summary["failed"] != 0:
            raise RuntimeError(f"unexpected import summary: {summary}")

        snapshot = admin_command(admin_port, "SNAPSHOT")
        distribution = validate_placements(
            snapshot, set(bucket_counts), len(backend_ports)
        )
        print(f"Balanced placements: {dict(sorted(distribution.items()))}")

        probes: dict[str, tuple[str, dict]] = {}
        for bucket, document in samples.items():
            try:
                terms, results = find_probe(cli, router_port, bucket, document)
            except RuntimeError:
                continue
            probes[bucket] = (terms, document)
            if len(probes) == min(5, len(samples)):
                break
        if len(probes) < min(3, len(samples)):
            raise RuntimeError(f"only {len(probes)} query probes succeeded")
        print(f"Validated reads through the router on {len(probes)} buckets")

        run(
            [
                str(cli),
                "--addr",
                f"127.0.0.1:{router_port}",
                "--password",
                PASSWORD,
                "consolidate",
            ]
        )
        print("Consolidation broadcast succeeded")

        bucket, source, target = select_migration(snapshot, probes)
        source_port = backend_ports[int(source.rsplit("-", 1)[1])]
        target_port = backend_ports[int(target.rsplit("-", 1)[1])]
        terms, probe = probes[bucket]
        before = query_documents(cli, router_port, bucket, terms)
        source_count = parse_count(
            channel_command(
                source_port, "ingest", f"COUNT {COLLECTION} {bucket}"
            )
        )
        target_count = parse_count(
            channel_command(
                target_port, "ingest", f"COUNT {COLLECTION} {bucket}"
            )
        )
        if source_count == 0 or target_count != 0:
            raise RuntimeError(
                f"invalid pre-migration counts: source={source_count}, target={target_count}"
            )

        admin_command(
            admin_port, f"MIGRATE START {COLLECTION} {bucket} {target}"
        )
        migration_file = runtime_dir / "migration.ndjson"
        migrated_documents = write_bucket_file(movies_ndjson, migration_file, bucket)
        migration_summary = import_documents(
            cli, router_port, migration_file, args.batch_documents
        )
        if migration_summary["imported"] != migrated_documents:
            raise RuntimeError(f"incomplete migration replay: {migration_summary}")
        admin_command(admin_port, f"MIGRATE CATCHUP {COLLECTION} {bucket}")
        admin_command(admin_port, f"MIGRATE CUTOVER {COLLECTION} {bucket}")

        after_cutover = query_documents(cli, router_port, bucket, terms)
        if before != after_cutover or not any(
            result.get("oid") == probe["oid"] for result in after_cutover
        ):
            raise RuntimeError("query results changed after migration cutover")

        admin_command(admin_port, f"MIGRATE DRAIN {COLLECTION} {bucket}")
        admin_command(admin_port, f"MIGRATE CLEANUP {COLLECTION} {bucket}")
        source_after = parse_count(
            channel_command(
                source_port, "ingest", f"COUNT {COLLECTION} {bucket}"
            )
        )
        target_after = parse_count(
            channel_command(
                target_port, "ingest", f"COUNT {COLLECTION} {bucket}"
            )
        )
        if source_after != 0 or target_after == 0:
            raise RuntimeError(
                f"invalid post-migration counts: source={source_after}, target={target_after}"
            )
        final_snapshot = admin_command(admin_port, "SNAPSHOT")
        placement = next(
            placement
            for placement in final_snapshot["placements"].values()
            if placement["bucket"] == bucket
        )
        if placement["primary"] != target or placement["state"] != "stable":
            raise RuntimeError(f"migration did not finish cleanly: {placement}")

        print(
            f"Migrated {bucket} from {source} to {target}; "
            f"{migrated_documents} documents replayed"
        )
        print(f"MovieDB router sharding test passed; logs: {runtime_dir}")
        processes.completed = True
    finally:
        processes.close()


if __name__ == "__main__":
    main()
