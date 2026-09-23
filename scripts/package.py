"""Build and verify a deterministic local Windows archive; never publishes it."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

TOOL = "verse-fmt"
TARGET = "x86_64-pc-windows-msvc"
DEFAULT_BINARY = Path("target") / TARGET / "release" / f"{TOOL}.exe"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def copy_new(source: Path, destination: Path) -> None:
    created = False
    try:
        with source.open("rb") as input_file, destination.open("xb") as output_file:
            created = True
            shutil.copyfileobj(input_file, output_file)
            output_file.flush()
            os.fsync(output_file.fileno())
    except Exception:
        if created:
            destination.unlink(missing_ok=True)
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", type=Path,
        help="unsupported: packages are rebuilt from the verified clean checkout",
    )
    parser.add_argument("--output-dir", type=Path, default=Path("target/packages"))
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    output_dir = args.output_dir.resolve()
    commit = subprocess.run(["git", "-C", str(root), "rev-parse", "--verify", "HEAD"],
                            capture_output=True, text=True, check=True).stdout.strip()
    dirty = subprocess.run(["git", "-C", str(root), "status", "--porcelain", "--untracked-files=normal"],
                           capture_output=True, text=True, check=True).stdout.strip()
    if dirty:
        raise SystemExit("package from a clean commit; commit source/docs before packaging")
    if args.binary is not None:
        raise SystemExit("custom --binary is unsupported; package the pinned build from this checkout")

    toolchain_match = re.search(
        r'^\s*channel\s*=\s*"([^"]+)"',
        (root / "rust-toolchain.toml").read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    if not toolchain_match:
        raise SystemExit("rust-toolchain.toml has no pinned channel")
    channel = toolchain_match.group(1)
    rustc = subprocess.run(
        ["rustup", "which", "--toolchain", channel, "rustc"],
        capture_output=True, text=True, check=True, timeout=30,
    ).stdout.strip()
    toolchain_bin = Path(rustc).parent
    cargo = toolchain_bin / ("cargo.exe" if os.name == "nt" else "cargo")
    build_env = os.environ.copy()
    build_env["PATH"] = str(toolchain_bin) + os.pathsep + build_env.get("PATH", "")
    build_env["RUSTUP_TOOLCHAIN"] = channel
    build_env["RUSTC"] = str(toolchain_bin / ("rustc.exe" if os.name == "nt" else "rustc"))
    build_env["RUSTDOC"] = str(toolchain_bin / ("rustdoc.exe" if os.name == "nt" else "rustdoc"))
    subprocess.run(
        [str(cargo), "build", "--release", "--locked", "--target", TARGET],
        cwd=root, env=build_env, check=True, timeout=1800,
    )
    binary = root / DEFAULT_BINARY
    if not binary.is_file():
        raise SystemExit(f"release binary not produced: {binary}")

    version_result = subprocess.run([str(binary), "--version"], capture_output=True, check=True)
    version_text = version_result.stdout.decode("utf-8", errors="strict").strip()
    match = re.fullmatch(rf"{TOOL} ([0-9A-Za-z.+-]+)", version_text)
    if not match:
        raise SystemExit(f"unexpected --version output: {version_text!r}")
    version = match.group(1)

    binary_bytes = binary.read_bytes()
    files = {f"{TOOL}.exe": binary_bytes}
    for name in ("README.md", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE"):
        files[name] = (root / name).read_bytes()
    for name in (
        "vendor/tree-sitter-verse/LICENSE",
        "vendor/tree-sitter-verse/LICENSE-tree-sitter",
    ):
        files[name] = (root / name).read_bytes()
    for path in sorted((root / "docs").rglob("*")):
        if path.is_file():
            files[path.relative_to(root).as_posix()] = path.read_bytes()
    build_info = (
        f"tool={TOOL}\nversion={version}\ntarget={TARGET}\nsource_commit={commit}\n"
        f"binary_sha256={sha256(binary_bytes)}\n"
    ).encode("ascii")
    files["BUILD-INFO.txt"] = build_info

    archive_name = f"{TOOL}-v{version}-{TARGET}.zip"
    output_dir.mkdir(parents=True, exist_ok=True)
    archive_path = output_dir / archive_name
    checksum_path = output_dir / f"{archive_name}.sha256"
    if archive_path.exists() or checksum_path.exists():
        raise SystemExit(f"refusing to overwrite existing package: {archive_path}")

    published = []
    with tempfile.TemporaryDirectory(prefix=f"{TOOL}-package-", dir=output_dir) as staging:
        stage = Path(staging)
        staged_archive = stage / archive_name
        staged_checksum = stage / f"{archive_name}.sha256"
        with zipfile.ZipFile(staged_archive, "w", compression=zipfile.ZIP_DEFLATED,
                             compresslevel=9, strict_timestamps=True) as archive:
            for name in sorted(files):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.create_system = 0
                archive.writestr(info, files[name], compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)

        archive_bytes = staged_archive.read_bytes()
        checksum = sha256(archive_bytes)
        staged_checksum.write_bytes(f"{checksum}  {archive_name}\n".encode("ascii"))
        with zipfile.ZipFile(staged_archive) as check:
            if check.testzip() is not None:
                raise RuntimeError("archive CRC verification failed")
            if set(check.namelist()) != set(files):
                raise RuntimeError("archive file list differs from staged files")
            if {name: check.read(name) for name in check.namelist()} != files:
                raise RuntimeError("archive contents differ from source inputs")
            with tempfile.TemporaryDirectory(prefix="verse-fmt-package-smoke-") as temporary:
                smoke_root = Path(temporary)
                check.extract(f"{TOOL}.exe", smoke_root)
                smoke_binary = smoke_root / f"{TOOL}.exe"
                system_root = os.environ.get("SystemRoot", r"C:\Windows")
                smoke_env = {
                    "SystemRoot": system_root,
                    "WINDIR": system_root,
                    "PATH": str(Path(system_root) / "System32") + os.pathsep + system_root,
                }
                smoke_version = subprocess.run([str(smoke_binary), "--version"], cwd=smoke_root,
                                                env=smoke_env, capture_output=True, check=True)
                if smoke_version.stdout.decode("utf-8", errors="strict").strip() != version_text:
                    raise RuntimeError("extracted binary version differs from packaged binary")
                smoke_source = smoke_root / "smoke.verse"
                smoke_source.write_bytes(b"A := 1\n")
                smoke_check = subprocess.run([str(smoke_binary), "--check", str(smoke_source)],
                                             cwd=smoke_root, env=smoke_env, capture_output=True)
                if smoke_check.returncode != 0:
                    raise RuntimeError("extracted binary failed direct --check smoke test")
        if staged_checksum.read_text(encoding="ascii") != f"{sha256(staged_archive.read_bytes())}  {archive_name}\n":
            raise RuntimeError("checksum verification failed")
        try:
            copy_new(staged_archive, archive_path)
            published.append(archive_path)
            copy_new(staged_checksum, checksum_path)
            published.append(checksum_path)
            if checksum_path.read_text(encoding="ascii") != f"{sha256(archive_path.read_bytes())}  {archive_name}\n":
                raise RuntimeError("published checksum verification failed")
        except Exception:
            for destination in published:
                destination.unlink(missing_ok=True)
            raise

    print(json.dumps({
        "tool": TOOL, "version": version, "target": TARGET, "sourceCommit": commit,
        "archive": str(archive_path), "archiveSha256": checksum,
        "entries": sorted(files), "extractedBinarySmoke": "passed with developer-tool PATH removed",
        "published": False,
    }, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
