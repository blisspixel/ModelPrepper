"""Render actual CLI output for documentation. Pillow is development-only."""
import hashlib
import json
import platform
import subprocess
import textwrap
from datetime import datetime, timezone
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "docs" / "assets"


def source_digest():
    files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", Path(__file__).resolve()]
    files += [p for p in (ROOT / "src").rglob("*.rs") if "tests" not in p.relative_to(ROOT / "src").parts]
    files += list((ROOT / "resources" / "licenses").glob("*.txt"))
    record = "".join(
        p.relative_to(ROOT).as_posix() + "\n" + p.read_bytes().decode("utf-8").replace("\r\n", "\n") + "\n"
        for p in sorted(files, key=lambda p: p.relative_to(ROOT).as_posix())
    )
    return hashlib.sha256(record.encode()).hexdigest()


def execute(binary, *args):
    return subprocess.run([str(binary), *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout.replace("\r\n", "\n")


def render(name, command, output):
    fonts = [Path("C:/Windows/Fonts/consola.ttf"), Path("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"), Path("/System/Library/Fonts/Menlo.ttc")]
    font_path = next((p for p in fonts if p.exists()), None)
    if font_path is None:
        raise SystemExit("A monospace system font is required for capture rendering")
    font = ImageFont.truetype(str(font_path), 20)
    lines = []
    for line in output.rstrip().splitlines():
        lines.extend(textwrap.wrap(line, width=98, replace_whitespace=False, drop_whitespace=False) or [""])
    image = Image.new("RGB", (1280, 150 + len(lines) * 28), "#101820")
    draw = ImageDraw.Draw(image)
    draw.text((40, 24), "ModelPrepper | actual CLI output | " + platform.system(), font=font, fill="#aebbc6")
    draw.line((40, 65, 1240, 65), fill="#33434f", width=1)
    draw.text((40, 86), "$ " + command, font=font, fill="#72c9e7")
    for index, line in enumerate(lines):
        draw.text((40, 128 + index * 28), line, font=font, fill="#e4edf3")
    image.save(ASSETS / (name + ".png"))


def main():
    subprocess.run(["cargo", "build", "--locked", "--release"], cwd=ROOT, check=True)
    binary = ROOT / "target" / "release" / ("modelprepper.exe" if platform.system() == "Windows" else "modelprepper")
    demo = ROOT / "target" / "docs-capture"
    demo.mkdir(parents=True, exist_ok=True)
    config = demo / "config.toml"
    if not config.exists():
        execute(binary, "config", "init", "--output", str(config), "--catalog", "catalog", "--volume", "models")
    execute(binary, "init", "--config", str(config))
    ASSETS.mkdir(parents=True, exist_ok=True)
    captures = []
    for name, command, args in [
        ("cli-help", "modelprepper --help", ["--help"]),
        ("cli-status", "modelprepper status --config target/docs-capture/config.toml", ["status", "--config", "target/docs-capture/config.toml"]),
    ]:
        raw = execute(binary, *args)
        (ASSETS / (name + ".txt")).write_bytes(raw.encode())
        displayed = json.dumps(json.loads(raw), indent=2, ensure_ascii=False) if name == "cli-status" else raw
        render(name, command, displayed)
        captures.append({"command": command, "output": name + ".txt", "image": name + ".png", "output_sha256": hashlib.sha256(raw.encode()).hexdigest(), "image_sha256": hashlib.sha256((ASSETS / (name + ".png")).read_bytes()).hexdigest()})
    manifest = {"schema_version": 1, "captured_at": datetime.now(timezone.utc).isoformat(), "platform": platform.system(), "version": execute(binary, "--version").strip(), "source_digest": source_digest(), "captures": captures}
    (ASSETS / "captures.json").write_bytes((json.dumps(manifest, indent=2) + "\n").encode())
    print("Captured current native CLI help and offline status.")


if __name__ == "__main__":
    main()
