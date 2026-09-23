"""Build and verify a deterministic local Windows archive; never publishes it."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import zipfile

TOOL = "verse-fmt"
TARGET = "x86_64-pc-windows-msvc"
DEFAULT_BINARY = Path("target") / TARGET / "release" / f"{TOOL}.exe"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--output-dir", type=Path, default=Path("target/packages"))
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    binary = args.binary.resolve(strict=True)
    output_dir = args.output_dir.resolve()
    if not binary.is_file():
        raise SystemExit(f"not a file: {binary}")
    if not args.binary.name.lower().endswith(".exe"):
        raise SystemExit("Windows package requires an .exe binary")

    version_result = subprocess.run([str(binary), "--version"], capture_output=True, check=True)
    version_text = version_result.stdout.decode("utf-8", errors="strict").strip()
    match = re.fullmatch(rf"{TOOL} ([0-9A-Za-z.+-]+)", version_text)
    if not match:
        raise SystemExit(f"unexpected --version output: {version_text!r}")
    version = match.group(1)

    commit = subprocess.run(["git", "-C", str(root), "rev-parse", "--verify", "HEAD"],
                            capture_output=True, text=True, check=True).stdout.strip()
    dirty = subprocess.run(["git", "-C", str(root), "status", "--porcelain", "--untracked-files=normal"],
                           capture_output=True, text=True, check=True).stdout.strip()
    if dirty:
        raise SystemExit("package from a clean commit; commit source/docs before packaging")

    binary_bytes = binary.read_bytes()
    files = {f"{TOOL}.exe": binary_bytes}
    for name in ("README.md", "LICENSE-MIT", "LICENSE-APACHE", "NOTICE"):
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

    with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED,
                         compresslevel=9, strict_timestamps=True) as archive:
        for name in sorted(files):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 0
            archive.writestr(info, files[name], compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)

    archive_bytes = archive_path.read_bytes()
    checksum = sha256(archive_bytes)
    checksum_path.write_bytes(f"{checksum}  {archive_name}\n".encode("ascii"))
    with zipfile.ZipFile(archive_path) as check:
        if check.testzip() is not None:
            raise RuntimeError("archive CRC verification failed")
        if set(check.namelist()) != set(files):
            raise RuntimeError("archive file list differs from staged files")
        if {name: check.read(name) for name in check.namelist()} != files:
            raise RuntimeError("archive contents differ from source inputs")
    if checksum_path.read_text(encoding="ascii") != f"{sha256(archive_path.read_bytes())}  {archive_name}\n":
        raise RuntimeError("checksum verification failed")

    print(json.dumps({
        "tool": TOOL, "version": version, "target": TARGET, "sourceCommit": commit,
        "archive": str(archive_path), "archiveSha256": checksum,
        "entries": sorted(files), "published": False,
    }, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
