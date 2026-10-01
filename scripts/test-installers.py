"""Exercise real native installer behavior offline, without changing user PATH."""

import argparse
import hashlib
import io
import platform
import subprocess
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from importlib.util import module_from_spec, spec_from_file_location

ROOT = Path(__file__).resolve().parent.parent
spec = spec_from_file_location(
    "package_release", ROOT / "scripts/package-native-release.py"
)
packager = module_from_spec(spec)
spec.loader.exec_module(packager)


class InstallerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.version = (
            subprocess.run(
                [str(BINARY), "--version"], check=True, capture_output=True, text=True
            )
            .stdout.strip()
            .split()[1]
        )

    def setUp(self):
        # Stay inside the workspace on restricted developer machines.
        self.temp = tempfile.TemporaryDirectory(
            prefix="installer-test-", dir=ROOT / "target"
        )
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.destination = self.root / "bin with spaces"
        self.archive, self.digest = packager.package(
            BINARY, "v" + self.version, self.root / "release"
        )
        self.executable = self.destination / (
            "modelprepper.exe" if platform.system() == "Windows" else "modelprepper"
        )

    def install(self, archive=None, digest=None, version=None, extras=()):
        archive, digest, version = (
            archive or self.archive,
            digest or self.digest,
            version or "v" + self.version,
        )
        if platform.system() == "Windows":
            command = [
                "powershell",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(ROOT / "install.ps1"),
                "-NoPath",
                "-InstallDir",
                str(self.destination),
                "-Version",
                version,
                "-ArchivePath",
                str(archive),
                "-Sha256",
                digest,
            ]
        else:
            command = [
                "sh",
                str(ROOT / "install.sh"),
                "--install-dir",
                str(self.destination),
                "--version",
                version,
                "--archive",
                str(archive),
                "--sha256",
                digest,
            ]
        return subprocess.run(
            command + list(extras), capture_output=True, text=True, timeout=30
        )

    def test_installs_and_upgrades_without_touching_configuration(self):
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.executable.read_bytes(), BINARY.read_bytes())
        self.assertEqual(
            (self.destination / "modelprepper.LICENSE").read_bytes(),
            (ROOT / "LICENSE").read_bytes(),
        )
        config = self.destination / "config.toml"
        config.write_text("retain me")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(config.read_text(), "retain me")
        self.assertEqual(list(self.destination.glob(".modelprepper*")), [])

    def test_checksum_mismatch_retains_previous_binary(self):
        self.assertEqual(self.install().returncode, 0)
        before = self.executable.read_bytes()
        result = self.install(digest="0" * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr.lower())
        self.assertEqual(self.executable.read_bytes(), before)

    def test_wrong_version_and_bad_arguments_do_not_install(self):
        for version in ["v999.0.0", "bad/version"]:
            result = self.install(version=version)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(self.executable.exists())
        self.assertNotEqual(self.install(digest="bad").returncode, 0)

    def test_archive_paths_and_extra_entries_are_rejected(self):
        for entry in [
            "../escape",
            "modelprepper.exe" if platform.system() != "Windows" else "other.exe",
        ]:
            if platform.system() == "Windows":
                bad = self.root / "bad.zip"
                with zipfile.ZipFile(bad, "w") as bundle:
                    bundle.writestr(entry, b"not an executable")
            else:
                bad = self.root / "bad.tar.gz"
                with tarfile.open(bad, "w:gz") as bundle:
                    info = tarfile.TarInfo(entry)
                    info.size = 3
                    bundle.addfile(info, io.BytesIO(b"bad"))
            digest = hashlib.sha256(bad.read_bytes()).hexdigest()
            result = self.install(archive=bad, digest=digest)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(self.executable.exists())
            self.assertFalse((self.root / "escape").exists())

    def test_packaging_refuses_to_replace_existing_release_asset(self):
        before = self.archive.read_bytes()
        with self.assertRaises(FileExistsError):
            packager.package(BINARY, "v" + self.version, self.root / "release")
        self.assertEqual(self.archive.read_bytes(), before)

    def test_linked_archive_entries_are_rejected_before_extraction(self):
        if platform.system() == "Windows":
            bad = self.root / "linked.zip"
            with zipfile.ZipFile(bad, "w") as bundle:
                link = zipfile.ZipInfo("modelprepper.exe")
                link.create_system = 3
                link.external_attr = 0o120777 << 16
                bundle.writestr(link, "../escape")
                bundle.writestr("LICENSE", b"fixture")
        else:
            bad = self.root / "linked.tar.gz"
            with tarfile.open(bad, "w:gz") as bundle:
                link = tarfile.TarInfo("modelprepper")
                link.type, link.linkname = tarfile.SYMTYPE, "../escape"
                bundle.addfile(link)
                license_entry = tarfile.TarInfo("LICENSE")
                license_entry.size = 7
                bundle.addfile(license_entry, io.BytesIO(b"fixture"))
        result = self.install(
            archive=bad, digest=hashlib.sha256(bad.read_bytes()).hexdigest()
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.executable.exists())

    def test_extra_archive_entries_are_rejected(self):
        if platform.system() == "Windows":
            bad = self.root / "extra.zip"
            with zipfile.ZipFile(bad, "w") as bundle:
                bundle.writestr("modelprepper.exe", BINARY.read_bytes())
                bundle.writestr("LICENSE", b"fixture")
                bundle.writestr("extra", b"unexpected")
        else:
            bad = self.root / "extra.tar.gz"
            with tarfile.open(bad, "w:gz") as bundle:
                for name in ["modelprepper", "LICENSE", "extra"]:
                    entry = tarfile.TarInfo(name)
                    entry.size = 3
                    bundle.addfile(entry, io.BytesIO(b"bad"))
        result = self.install(
            archive=bad, digest=hashlib.sha256(bad.read_bytes()).hexdigest()
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.executable.exists())


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    BINARY = args.binary.resolve()
    unittest.main(argv=[__file__], verbosity=2)
