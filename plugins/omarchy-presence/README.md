# ModelPrepper presence

Optional Omarchy Quattro bar chip. It shows whether the configured vault disks are mounted, offline, missing an identity marker, or carrying a different disk. The panel adds the snapshot time, the selected config path, each volume's identity state and free space, and catalog custody rows.

The chip is a view of `modelprepper status`. It is not a second custody verdict. Catalog rows are historical provenance. They are not a fresh hash, not a mounted-disk check, and not evidence that a model is safe or can run offline. The chip never says safe or verified. A runtime coverage count is not shown; that would need the partial and unchecked rules from `local list`, and this widget does not run that command.

`status` acquires the vault writer lock and can update SQLite journal bookkeeping. It does not contact a publisher, hash weights, or change preserved bytes. This widget does not call any other command.

## What this repository does not do

This folder is source only. ModelPrepper does not copy it into `~/.config/omarchy/plugins/`, does not run `omarchy plugin add`, and does not publish it to the Omarchy marketplace. [Issue #19](https://github.com/blisspixel/ModelPrepper/issues/19) does not authorize installation or marketplace publication. The CLI does not require the widget.

The license is the repository [MIT license](../../LICENSE). There is no separate plugin license.

## Settings

The bar entry in `~/.config/omarchy/shell.json` holds two strings:

| Key | Meaning |
| --- | --- |
| `config` | Absolute path of the ModelPrepper config file. Passed as one argument. |
| `executable` | Path of the `modelprepper` binary. One argv element, not a shell command. The default is `modelprepper` on `PATH`. |

No tokens, account names, or other secrets belong in these settings. A null byte is rejected and nothing is launched. Leading and trailing whitespace is removed. Other characters, including spaces, quotes, and newlines, stay inside that one argument.

## Commands

A refresh uses this argument list and no shell:

```text
<executable> status --json --config <config>
```

Copy and Open inspection use:

```text
<executable> status --config <config>
```

Copy writes that list as JSON. It is not a shell command. Under Wayland, Quickshell's clipboard stays empty unless a shell window is focused. Open inspection runs the same list through Quickshell `Process.exec`, which does not invoke a shell. One operation runs at a time. A second monitor, a timer tick, or a click during that operation is coalesced into at most one follow-up. The output cap is 256 KiB. The deadline is 4 seconds. A result from an older config, a disabled widget, a timed-out process, or a destroyed widget is ignored.

The last successful snapshot stays on screen when a later read fails or goes stale. It is dimmed and keeps its original time. These stay distinct: unknown, writer busy, missing config, unreadable config, invalid config, offline disk, missing identity marker, replaced disk, malformed status, missing executable, timeout, and stale data.

The refresh interval is 60 seconds. The snapshot is stale at 120 seconds. The age timer does not start a process. There is no background service and no second Quickshell process.

## Checks

Node evaluates `Model.js` directly, including the mounted, offline, missing-marker, replaced-disk, writer-busy, invalid, malformed, missing-executable, hostile-path, timeout, stale, and config-change fixtures. Those tests assert the launched arguments. They do not assert that `status` itself skips SQLite journal bookkeeping. That limit is in [contracts](../../docs/contracts.md).

This machine cannot run an Omarchy session. These checks were not run: `omarchy plugin validate`, `qmllint`, click, Escape, disable, re-enable, shell restart, and removal. Do not treat the widget as accepted on the bar until those happen in a separately authorized session.
