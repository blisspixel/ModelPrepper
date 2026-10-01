"""Create the installer's native archive and checksum contract using the stdlib."""

import argparse
import gzip
import hashlib
import io
import platform
import re
import subprocess
import tarfile
import zipfile
from pathlib import Path


def native_target():
    machine = platform.machine().lower()
    cpu = {
        "amd64": "x86_64",
        "x86_64": "x86_64",
        "arm64": "aarch64",
        "aarch64": "aarch64",
    }.get(machine)
    suffix = {
        "Windows": "pc-windows-msvc",
        "Darwin": "apple-darwin",
        "Linux": "unknown-linux-gnu",
    }.get(platform.system())
    if not cpu or not suffix or (platform.system() == "Windows" and cpu != "x86_64"):
        raise ValueError("this native platform is not supported")
    return cpu + "-" + suffix


def package(binary, version, output):
    if not re.fullmatch(
        r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", version
    ):
        raise ValueError("version must be a release tag such as v0.1.0")
    observed = subprocess.run(
        [str(binary.resolve()), "--version"], check=True, capture_output=True, text=True
    ).stdout.strip()
    if observed != "modelprepper " + version[1:]:
        raise ValueError("native binary version differs from release tag")
    data = binary.read_bytes()
    if len(data) > 128 * 1024 * 1024:
        raise ValueError("native executable exceeds the 128 MiB installer limit")
    target = native_target()
    windows = platform.system() == "Windows"
    name = f"modelprepper-{version}-{target}" + (".zip" if windows else ".tar.gz")
    entry = "modelprepper.exe" if windows else "modelprepper"
    license_bytes = (Path(__file__).resolve().parent.parent / "LICENSE").read_bytes()
    files = [(entry, data, 0o755), ("LICENSE", license_bytes, 0o644)]
    output.mkdir(parents=True, exist_ok=True)
    archive = output / name
    # Exclusive creation prevents replacing bytes under an existing asset name.
    with archive.open("xb") as file:
        if windows:
            with zipfile.ZipFile(file, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
                for name_in_archive, contents, mode in files:
                    info = zipfile.ZipInfo(name_in_archive, (1980, 1, 1, 0, 0, 0))
                    info.create_system = 3
                    info.external_attr = (0o100000 | mode) << 16
                    info.compress_type = zipfile.ZIP_DEFLATED
                    bundle.writestr(info, contents)
        else:
            with gzip.GzipFile(
                filename="", fileobj=file, mode="wb", mtime=0
            ) as compressed:
                with tarfile.open(
                    fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT
                ) as bundle:
                    for name_in_archive, contents, mode in files:
                        info = tarfile.TarInfo(name_in_archive)
                        info.size, info.mode, info.mtime = len(contents), mode, 0
                        bundle.addfile(info, io.BytesIO(contents))
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    manifest = output / "SHA256SUMS"
    entries = {}
    if manifest.exists():
        for line in manifest.read_text(encoding="ascii").splitlines():
            match = re.fullmatch(
                r"([0-9a-f]{64})  (modelprepper-v[0-9A-Za-z._-]+\.(?:zip|tar\.gz))",
                line,
            )
            if not match or match[2] in entries:
                raise ValueError("existing release checksum manifest is invalid")
            entries[match[2]] = match[1]
    entries[name] = digest
    manifest.write_text(
        "".join(f"{entries[n]}  {n}\n" for n in sorted(entries)),
        encoding="ascii",
        newline="\n",
    )
    return archive, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    archive, digest = package(args.binary, args.version, args.output_dir)
    print(f"{digest}  {archive.name}")


if __name__ == "__main__":
    main()
