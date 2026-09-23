"""Reproducible, startup-inclusive Windows benchmark for both release CLIs."""
import argparse
import ctypes
import ctypes.wintypes
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import threading
import time

class MemoryCountersEx(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.wintypes.DWORD), ("page_fault_count", ctypes.wintypes.DWORD),
        ("peak_working_set", ctypes.c_size_t), ("working_set", ctypes.c_size_t),
        ("peak_paged_pool", ctypes.c_size_t), ("paged_pool", ctypes.c_size_t),
        ("peak_nonpaged_pool", ctypes.c_size_t), ("nonpaged_pool", ctypes.c_size_t),
        ("pagefile_usage", ctypes.c_size_t), ("peak_pagefile_usage", ctypes.c_size_t),
        ("private_usage", ctypes.c_size_t),
    ]

def private_bytes(pid):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    kernel.OpenProcess.argtypes = [ctypes.wintypes.DWORD, ctypes.wintypes.BOOL, ctypes.wintypes.DWORD]
    kernel.OpenProcess.restype = ctypes.wintypes.HANDLE
    kernel.CloseHandle.argtypes = [ctypes.wintypes.HANDLE]
    psapi.GetProcessMemoryInfo.argtypes = [ctypes.wintypes.HANDLE, ctypes.POINTER(MemoryCountersEx), ctypes.wintypes.DWORD]
    psapi.GetProcessMemoryInfo.restype = ctypes.wintypes.BOOL
    handle = kernel.OpenProcess(0x1000, False, pid) # PROCESS_QUERY_LIMITED_INFORMATION
    if not handle:
        return None
    try:
        counters = MemoryCountersEx()
        counters.cb = ctypes.sizeof(counters)
        if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
            return None
        return int(counters.private_usage)
    finally:
        kernel.CloseHandle(handle)

def source_of_size(size):
    prefix, suffix = b'A := "', b'"\n'
    if size < len(prefix) + len(suffix):
        raise ValueError("benchmark source size too small")
    return prefix + b'x' * (size - len(prefix) - len(suffix)) + suffix

def record_hashes(root):
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(root.rglob("*.verse")) if p.is_file()}

def run_sample(command, cwd):
    start = time.perf_counter()
    process = subprocess.Popen(command, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    stderr_prefix = bytearray()

    def drain_stderr():
        # Keep draining after the diagnostic prefix is full, or a noisy child
        # can block on the pipe while the benchmark waits for it to exit.
        while True:
            chunk = process.stderr.read1(8192)
            if not chunk:
                break
            remaining = 2048 - len(stderr_prefix)
            if remaining > 0:
                stderr_prefix.extend(chunk[:remaining])

    stderr_reader = threading.Thread(target=drain_stderr, daemon=True)
    stderr_reader.start()
    peak = 0
    while process.poll() is None:
        current = private_bytes(process.pid)
        if current is not None:
            peak = max(peak, current)
        time.sleep(0.002)
    process.wait()
    stderr_reader.join()
    process.stderr.close()
    elapsed = time.perf_counter() - start
    return {"seconds": elapsed, "exit_code": process.returncode,
            "peak_private_bytes_sampled": peak or None,
            "stderr": bytes(stderr_prefix).decode("utf-8", errors="replace")[:500]}

def optional_run(command, **kwargs):
    """Run an optional metadata probe; missing executables mean unavailable metadata."""
    try:
        return subprocess.run(command, **kwargs)
    except FileNotFoundError:
        return None

def git_head(path):
    root = optional_run(["git", "-C", str(path), "rev-parse", "--show-toplevel"],
                        capture_output=True, text=True)
    if root is None or root.returncode != 0:
        return None
    result = optional_run(["git", "-C", root.stdout.strip(), "rev-parse", "--verify", "HEAD"],
                          capture_output=True, text=True)
    return result.stdout.strip() if result is not None and result.returncode == 0 else None

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--formatter", required=True, type=Path)
    parser.add_argument("--linter", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    if os.name != "nt":
        raise SystemExit("This benchmark measures Windows process memory and is Windows-only")
    if args.runs < 3:
        raise SystemExit("use at least 3 warm runs")
    binaries = {"formatter": args.formatter.resolve(strict=True), "linter": args.linter.resolve(strict=True)}
    worktree = Path(__file__).resolve().parents[1]
    versions = {}
    for name, binary in binaries.items():
        versions[name] = subprocess.run([str(binary), "--version"], capture_output=True, text=True, check=True).stdout.strip()
    toolchain = optional_run(["rustup", "run", "1.97.0", "rustc", "-Vv"],
                             capture_output=True, text=True)
    pwsh = optional_run(["pwsh", "-NoProfile", "-Command", "$PSVersionTable.PSVersion.ToString()"],
                        capture_output=True, text=True)
    report = {
        "schemaVersion": 1,
        "benchmark": "startup-inclusive local Windows CLI; self-authored protected-literal inputs",
        "machine": {"os": platform.platform(), "windows_version": platform.win32_ver(),
                    "architecture": platform.machine(), "processor": platform.processor(),
                    "processor_identifier": os.environ.get("PROCESSOR_IDENTIFIER"),
                    "python": sys.version,
                    "powershell": pwsh.stdout.strip() if pwsh is not None and pwsh.returncode == 0 else None,
                    "rust_1_97_verbose": toolchain.stdout if toolchain is not None and toolchain.returncode == 0 else None},
        "revisions": {"formatter": git_head(binaries["formatter"].parent),
                      "linter": git_head(binaries["linter"].parent)},
        "binaries": {name: {"path": str(path), "version": versions[name],
                            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                     for name, path in binaries.items()},
        "runs_per_case": args.runs,
        "cases": [],
    }
    with tempfile.TemporaryDirectory(prefix="verse-cli-benchmark-") as temporary:
        base = Path(temporary)
        cases = []
        for size in (1024, 100 * 1024, 1024 * 1024):
            directory = base / f"single-{size}"
            directory.mkdir()
            (directory / "source.verse").write_bytes(source_of_size(size))
            cases.append((f"single-file-{size}-bytes", directory, 1, size))
        for count, size, label in ((100, 10 * 1024, "100-files-about-1MiB"),
                                   (1000, 10 * 1024, "1000-files-about-10MiB")):
            directory = base / label
            directory.mkdir()
            payload = source_of_size(size)
            for index in range(count):
                (directory / f"file-{index:04}.verse").write_bytes(payload)
            cases.append((label, directory, count, count * size))

        for label, directory, file_count, source_bytes in cases:
            before = record_hashes(directory)
            for tool, binary in binaries.items():
                command = ([str(binary), "--check", "."] if tool == "formatter"
                           else [str(binary), "--output-format", "json", "."])
                first = run_sample(command, directory)
                warm = [run_sample(command, directory) for _ in range(args.runs)]
                all_samples = [first, *warm]
                bad = [sample for sample in all_samples if sample["exit_code"] != 0]
                if bad:
                    raise RuntimeError(f"{label}/{tool} failed: {bad[0]}")
                times = [item["seconds"] for item in warm]
                peaks = [item["peak_private_bytes_sampled"] for item in all_samples
                         if item["peak_private_bytes_sampled"] is not None]
                result = {
                    "tool": tool, "command": command, "file_count": file_count,
                    "source_bytes": source_bytes, "first_run_seconds": first["seconds"],
                    "warm_seconds": times, "warm_median_seconds": statistics.median(times),
                    "sampled_peak_private_bytes": max(peaks) if peaks else None,
                    "all_exit_codes": [item["exit_code"] for item in all_samples],
                }
                report["cases"].append({"name": label, **result})
                print(f"{label:27} {tool:9} first={first['seconds']:.3f}s warm-median={result['warm_median_seconds']:.3f}s sampled-private={result['sampled_peak_private_bytes']}")
            if record_hashes(directory) != before:
                raise RuntimeError(f"read-only benchmark modified sources in {label}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"report: {args.output}")

if __name__ == "__main__":
    main()
