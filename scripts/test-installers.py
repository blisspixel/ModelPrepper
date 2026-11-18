"""Exercise real native installer behavior offline, without changing user PATH."""

import argparse
import hashlib
import io
import json
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

    def cli(self, *args):
        result = subprocess.run(
            [str(self.executable), "--json", *args],
            cwd=self.root,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_installed_cli_runs_a_first_offline_vault_plan(self):
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)

        config = "first-vault.toml"
        self.cli(
            "config",
            "init",
            "--output",
            config,
            "--catalog",
            "catalog",
            "--volume",
            "models",
        )
        self.cli("config", "check", "--config", config)
        initialized = self.cli("init", "--config", config)
        status = self.cli("status", "--config", config)
        self.assertEqual(status["vault_id"], initialized["vault_id"])
        self.assertEqual(status["volumes"][0]["state"], "mounted")
        plan = self.cli(
            "plan",
            "--config",
            config,
            "--inventory",
            str(ROOT / "tests/fixtures/inventory.json"),
            "--available-bytes",
            "500000000000",
        )
        self.assertEqual(plan["payload_bytes"], 5504)
        self.assertFalse(plan["transfer_authorized"])
        self.assertIsNone(plan["workspace_bytes"])
        self.assertIn("local_inventory_untrusted", plan["blockers"])
        self.assertEqual(self.cli("status", "--config", config)["preserved"], [])

    def test_installed_cli_recovers_from_the_backup_volume_alone(self):
        installed = self.install()
        self.assertEqual(installed.returncode, 0, installed.stderr)
        config = "vault.toml"
        self.cli(
            "config",
            "init",
            "--output",
            config,
            "--catalog",
            "catalog",
            "--volume",
            "primary",
        )
        config_path = self.root / config
        lines = config_path.read_text().splitlines()
        lines = [
            "min_free_bytes = 1048576" if line.startswith("min_free_bytes =") else line
            for line in lines
        ]
        config_path.write_text(
            "\n".join(lines) + '\n[[volumes]]\nlabel = "backup"\npath = "backup"\n'
        )
        self.cli("init", "--config", config)
        incoming = self.root / "incoming"
        incoming.mkdir()
        (incoming / "weights.txt").write_bytes(b"native recovery fixture")
        custody = self.cli(
            "import",
            "--config",
            config,
            "--path",
            "incoming",
            "--repo",
            "local/fixture",
            "--revision",
            "a" * 40,
            "--volume",
            "primary",
        )
        plan = custody["plan_id"]
        self.cli("replica", "--config", config, "--plan", plan, "--volume", "backup")
        for path in ("primary", "incoming"):
            (self.root / path).rename(self.root / ("unavailable-" + path))
        verified = self.cli(
            "verify-sealed", "--config", config, "--plan", plan, "--volume", "backup"
        )
        self.assertTrue(verified["matched"])
        self.assertEqual(verified["copy_kind"], "replica")
        receipt = self.cli(
            "catalog-backup", "--config", config, "--output", "snapshot.sqlite3"
        )
        snapshot = self.root / "snapshot.sqlite3"
        self.assertEqual(
            receipt["sha256"], hashlib.sha256(snapshot.read_bytes()).hexdigest()
        )
        self.assertEqual(receipt["size_bytes"], snapshot.stat().st_size)
        (self.root / "catalog").rename(self.root / "unavailable-catalog")

        recovery = "recovery.toml"
        self.cli(
            "config",
            "init",
            "--output",
            recovery,
            "--catalog",
            "fresh-catalog",
            "--volume",
            "backup",
        )
        path = self.root / recovery
        path.write_text(path.read_text().replace('"primary"', '"backup"'))
        backup = self.root / "backup"
        detached = self.root / "detached-backup"
        backup.rename(detached)
        unavailable = subprocess.run(
            [str(self.executable), "--json", "rebuild", "--config", recovery],
            cwd=self.root,
            text=True,
            capture_output=True,
            timeout=30,
        )
        self.assertNotEqual(unavailable.returncode, 0)
        self.assertEqual(
            json.loads(unavailable.stderr)["error"]["code"], "volume_identity_missing"
        )
        self.assertFalse((self.root / "fresh-catalog" / "catalog.sqlite3").exists())
        self.assertFalse(backup.exists())
        detached.rename(backup)
        rebuilt = self.cli("rebuild", "--config", recovery)
        self.assertEqual((rebuilt["custody_count"], rebuilt["replica_count"]), (1, 0))
        self.assertEqual(rebuilt["damaged"], [])
        self.assertEqual(rebuilt["conflicts"], [])
        self.assertFalse(rebuilt["approvals_transferred"])
        recovered = self.cli("status", "--config", recovery)["preserved"][0]
        self.assertEqual(
            (recovered["plan_id"], recovered["provenance"]), (plan, "local-baseline")
        )
        report = self.cli(
            "verify-sealed", "--config", recovery, "--plan", recovered["plan_id"]
        )
        self.assertTrue(report["matched"])
        self.assertEqual(
            (report["copy_kind"], report["volume_label"]), ("custody", "backup")
        )
        payload = (
            self.root
            / "backup"
            / "preserved"
            / "local"
            / "fixture"
            / ("a" * 40)
            / "weights.txt"
        )
        payload.write_bytes(b"damaged")
        damaged = subprocess.run(
            [
                str(self.executable),
                "--json",
                "verify-sealed",
                "--config",
                recovery,
                "--plan",
                plan,
            ],
            cwd=self.root,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertNotEqual(damaged.returncode, 0)
        self.assertFalse(json.loads(damaged.stdout)["matched"])
        partial = subprocess.run(
            [str(self.executable), "--json", "rebuild", "--config", recovery],
            cwd=self.root,
            capture_output=True,
            text=True,
            timeout=30,
        )
        self.assertNotEqual(partial.returncode, 0)
        report = json.loads(partial.stdout)
        self.assertEqual(report["status"], "partial")
        self.assertFalse(report["ok"])
        self.assertTrue(report["damaged"])
        self.assertEqual(report["unconfirmed_copies"], [f"backup:{plan}"])
        self.assertEqual(report["custody_count"], 1)
        for name in ("catalog", "primary", "incoming"):
            self.assertFalse((self.root / name).exists())

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

    def test_truncated_archive_retains_previous_installation(self):
        self.assertEqual(self.install().returncode, 0)
        before = self.executable.read_bytes()
        bad = self.root / self.archive.name
        bad.write_bytes(self.archive.read_bytes()[:100])
        result = self.install(
            archive=bad, digest=hashlib.sha256(bad.read_bytes()).hexdigest()
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.executable.read_bytes(), before)
        self.assertEqual(list(self.destination.glob(".modelprepper*")), [])

    def test_oversized_entries_are_rejected_before_installation(self):
        limit = 134217728
        bad = self.root / self.archive.name
        if platform.system() == "Windows":
            with zipfile.ZipFile(bad, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
                bundle.writestr("modelprepper.exe", BINARY.read_bytes())
                with bundle.open("LICENSE", "w") as entry:
                    for _ in range(128):
                        entry.write(bytes(1024 * 1024))
                    entry.write(b"x")
        else:

            class ZeroReader:
                def read(self, size):
                    return bytes(size)

            with tarfile.open(bad, "w:gz") as bundle:
                binary = tarfile.TarInfo("modelprepper")
                binary.size = BINARY.stat().st_size
                with BINARY.open("rb") as stream:
                    bundle.addfile(binary, stream)
                oversized = tarfile.TarInfo("LICENSE")
                oversized.size = limit + 1
                bundle.addfile(oversized, ZeroReader())
        result = self.install(
            archive=bad, digest=hashlib.sha256(bad.read_bytes()).hexdigest()
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("128 MiB", result.stderr)
        self.assertFalse(self.executable.exists())

    def test_destination_file_collision_does_not_replace_user_data(self):
        self.destination.write_bytes(b"unrelated user data")
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.destination.read_bytes(), b"unrelated user data")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    BINARY = args.binary.resolve()
    unittest.main(argv=[__file__], verbosity=2)
