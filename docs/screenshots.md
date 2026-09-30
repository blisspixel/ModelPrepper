# Current CLI captures

Images under assets render actual command output from the native release executable. They are not generated product mockups or a simulated GUI.

| Image | Captured command |
| --- | --- |
| [Help](assets/cli-help.png) | modelprepper --help |
| [Offline status](assets/cli-status.png) | modelprepper status for a newly initialized demonstration vault |

Status is formatted as JSON without changing field values. The demonstration vault lives under target and contains no weights. Source fingerprint and output/image hashes are recorded in [the manifest](assets/captures.json). No capture claims model preservation or offline loading.

## Regenerate

```text
cargo build --locked --release
python scripts/capture-cli.py
npm run check:captures
```

The optional renderer uses Pillow and a monospace system font. These are documentation tools, not runtime dependencies. CI validates committed captures without installing Pillow or taking screenshots.

Regenerate after application source, embedded license templates, package/lockfile, or the renderer changes. Freshness checks compare source, raw output, and image hashes. Visual review is still required.
