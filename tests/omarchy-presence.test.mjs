import { strict as assert } from 'node:assert';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createContext, runInContext } from 'node:vm';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const pluginDir = join(root, 'plugins', 'omarchy-presence');
const fixtureDir = join(root, 'tests', 'fixtures', 'presence');
const modelPath = join(pluginDir, 'Model.js');
const source = readFileSync(modelPath, 'utf8');

const sandbox = {};
createContext(sandbox);
runInContext(source.replace(/^\.pragma library\r?\n/, ''), sandbox, { filename: modelPath });
const api = sandbox.api;

const bannedChip = /safe|verified/i;
const forbiddenTokens = [
  'init', 'adopt', 'decide', 'resolve', 'propose', 'proposals', 'verify-local', 'plan', 'config', 'download', 'pull',
];

function fixture(name) {
  return readFileSync(join(fixtureDir, name), 'utf8');
}

function ready(executable = 'modelprepper', configPath = '/vault/config.toml', now = 1_000) {
  return api.configure(api.createSession(0), executable, configPath, true, now);
}

function finish(step, body, now, exitCode = 0, stderr = '') {
  return api.noteResult(step.session, {
    generation: step.launch.generation,
    now,
    exitCode,
    stdout: body,
    stderr,
    spawnError: '',
  });
}

function args(argv) {
  return Array.from(argv);
}

function assertStatusLaunch(launch, executable, configPath) {
  assert.ok(launch);
  assert.equal(launch.kind, 'status');
  assert.deepEqual(args(launch.argv), [executable, 'status', '--json', '--config', configPath]);
  assert.equal(api.argvOk(launch.argv, 'status'), true);
  assert.equal(launch.timeoutMs, api.TIMEOUT_MS);
  assert.equal(launch.outputCap, api.OUTPUT_CAP);
  for (const token of launch.argv.slice(1)) assert.equal(forbiddenTokens.includes(token), false);
}

test('the widget and the tests share one pragma library', () => {
  assert.equal(source.startsWith('.pragma library\n') || source.startsWith('.pragma library\r\n'), true);
  assert.equal(api.projectionVersion(), 1);
  assert.equal(source.includes('child_process'), false);
  assert.equal(source.includes('https://'), false);
  assert.equal(source.includes('http://'), false);
});

test('status and inspection stay literal argv arrays', () => {
  const executable = '/opt/My Models/modelprepper';
  const configPath = '/tmp/$(touch pwned)/config.toml\n; rm -rf / "quoted"';
  const step = ready(executable, configPath, 5);
  assertStatusLaunch(step.launch, executable, configPath);
  assert.equal(step.launch.argv.length, 5);
  assert.equal(step.view.inspectArgv.length, 4);
  assert.deepEqual(args(step.view.inspectArgv), [executable, 'status', '--config', configPath]);
  assert.equal(step.view.copyText.includes('\n'), false);
  assert.deepEqual(JSON.parse(step.view.copyText), args(step.view.inspectArgv));
  assert.notEqual(step.view.copyText, args(step.view.inspectArgv).join(' '));
  assert.equal(step.view.configPath.includes('\n'), false);
  assert.equal(step.view.configPath.includes('$(touch pwned)'), true);

  const longPath = `/vault/${'n'.repeat(4000)}/config.toml`;
  const longStep = ready('modelprepper', longPath, 6);
  assert.equal(longStep.launch.argv[4], longPath);
  assert.equal(longStep.launch.argv.length, 5);

  const nul = api.configure(api.createSession(0), 'modelprepper', '/tmp/a\0.toml', true, 7);
  assert.equal(nul.launch, null);
  assert.equal(nul.view.chip, 'invalid');
  assert.equal(nul.view.failureKind, 'invalid_config');
  assert.equal(nul.view.copyText, '');

  const same = api.configure(step.session, executable, configPath, true, 8);
  assert.equal(same.launch, null);
  assert.equal(same.session.generation, step.session.generation);

  const trimmed = ready('  modelprepper  ', '  /vault/config.toml  ', 9);
  assert.deepEqual(args(trimmed.launch.argv), ['modelprepper', 'status', '--json', '--config', '/vault/config.toml']);
});

test('free space uses the CLI decimal and binary pair', () => {
  assert.equal(api.spaceLabel(0), '0 B (0 B) free');
  assert.equal(api.spaceLabel(999), '999 B (999 B) free');
  assert.equal(api.spaceLabel(1000), '1.0 KB (1000 B) free');
  assert.equal(api.spaceLabel(1536), '1.5 KB (1.5 KiB) free');
  assert.equal(api.spaceLabel(463856467968), '463.9 GB (432.0 GiB) free');
  assert.equal(api.spaceLabel(null), 'space not measured');
  assert.equal(api.spaceLabel(1.5), 'space not measured');
  assert.equal(api.spaceLabel(9007199254740993), 'space not measured');
});

test('mounted status is presence, not a safety verdict', () => {
  const step = ready();
  const done = finish(step, fixture('mounted.json'), 1_200);
  assert.equal(done.view.chip, 'present');
  assert.equal(done.view.phase, 'current');
  assert.equal(done.view.dimmed, false);
  assert.equal(done.view.meaning, 'Every configured disk is mounted.');
  assert.equal(done.view.vaultState, 'ready');
  assert.equal(done.view.vaultId, '8397c2c3');
  assert.equal(done.view.volumes.length, 1);
  assert.equal(done.view.volumes[0].label, 'primary');
  assert.equal(done.view.volumes[0].state, 'mounted');
  assert.equal(done.view.volumes[0].space, '463.9 GB (432.0 GiB) free');
  assert.equal(done.view.volumes[0].path, '/vault/models');
  assert.equal(done.view.snapshotLine.includes('just now'), true);
  assert.equal(done.view.preserved.length, 1);
  assert.equal(done.view.preserved[0].repo, 'Qwen/Qwen2.5-0.5B-Instruct');
  assert.equal(done.view.preserved[0].revision, '7ae55760');
  assert.equal(done.view.preserved[0].provenance, 'upstream-verified (local source)');
  assert.equal(done.view.preservedNote, 'Historical catalog provenance. Not a fresh integrity check.');
  assert.equal(bannedChip.test(done.view.chip), false);
  assert.equal(bannedChip.test(done.view.meaning), false);
  assert.equal(bannedChip.test(done.view.preservedNote), false);
  assert.equal(done.launch, null);
});

test('removed, missing, empty, and unfinished disks stay distinct', () => {
  const cases = [
    ['offline.json', 'offline', 'offline', 'space not measured'],
    ['missing-marker.json', 'missing', 'identity_missing', 'space not measured'],
    ['empty-vault.json', 'empty', null, null],
    ['blocked.json', 'blocked', 'mounted', '1.0 KB (1000 B) free'],
  ];
  for (const [name, chip, state, space] of cases) {
    const done = finish(ready(), fixture(name), 1_300);
    assert.equal(done.view.chip, chip, name);
    assert.equal(done.view.phase, 'current', name);
    if (state) {
      assert.equal(done.view.volumes[0].state, state, name);
      assert.equal(done.view.volumes[0].space, space, name);
    } else {
      assert.equal(done.view.volumes.length, 0, name);
    }
    assert.equal(bannedChip.test(done.view.chip + done.view.meaning), false, name);
  }

  const replaced = finish(ready(), fixture('replaced-disk.json'), 1_400);
  assert.equal(replaced.view.chip, 'mismatch');
  assert.equal(replaced.view.volumes[0].state, 'offline');
  assert.equal(replaced.view.volumes[1].state, 'identity_mismatch');
  assert.equal(replaced.view.volumes[1].path, '/media/replaced');
  assert.equal(replaced.view.volumes[1].space, 'space not measured');
  assert.equal(replaced.view.preserved[0].provenance, 'upstream-verified (local source)');
  assert.equal(replaced.view.chip, 'mismatch');
});

test('a hostile volume path cannot rewrite the state line', () => {
  const body = JSON.stringify({
    schema_version: 1,
    state: 'ready',
    vault_id: '8397c2c3aaaaaaaaaaaaaaaaaaaaaaaa',
    volumes: [{
      label: 'primary\nmismatch',
      volume_id: 'volume',
      path: `/vault/${'p'.repeat(500)}\nmodels`,
      state: 'mounted',
      available_bytes: 0,
    }],
    preserved: [],
  });
  const done = finish(ready(), body, 1_500);
  assert.equal(done.view.chip, 'present');
  assert.equal(done.view.volumes[0].state, 'mounted');
  assert.equal(done.view.volumes[0].label.includes('\n'), false);
  assert.equal(done.view.volumes[0].path.includes('\n'), false);
  assert.equal(done.view.volumes[0].path.includes('p'.repeat(100)), true);
  assert.equal(done.view.volumes[0].space, '0 B (0 B) free');
});

test('writer busy keeps the last good snapshot and its original time', () => {
  const mounted = finish(ready(), fixture('mounted.json'), 2_000);
  const again = api.noteTick(mounted.session, 62_000, false);
  assertStatusLaunch(again.launch, 'modelprepper', '/vault/config.toml');
  const busy = finish(again, '', 63_000, 1, fixture('writer-busy.json'));
  assert.equal(busy.view.chip, 'present');
  assert.equal(busy.view.dimmed, true);
  assert.equal(busy.view.phase, 'failed');
  assert.equal(busy.view.failureKind, 'writer_busy');
  assert.equal(busy.view.failureCode, 'writer_busy');
  assert.equal(busy.view.failureText, 'another process owns the vault writer lock');
  assert.equal(busy.view.capturedAt, 2_000);
  assert.equal(busy.view.volumes[0].state, 'mounted');
  assert.equal(busy.view.ageMs, 61_000);
  assert.equal(busy.launch, null);
});

test('config, schema, and unknown failures do not become presence', () => {
  const missing = finish(ready(), '', 1_100, 1, fixture('missing-config.json'));
  assert.equal(missing.view.chip, 'config');
  assert.equal(missing.view.failureKind, 'missing_config');
  assert.equal(missing.view.volumes.length, 0);

  const invalid = finish(ready(), '', 1_100, 1, fixture('invalid-config.json'));
  assert.equal(invalid.view.chip, 'invalid');
  assert.equal(invalid.view.failureKind, 'invalid_config');

  const unsafe = finish(ready(), '', 1_100, 1, JSON.stringify({
    schema_version: 1,
    error: { code: 'unsafe_vault_path', message: 'symlinks and reparse points cannot own vault paths' },
  }));
  assert.equal(unsafe.view.chip, 'invalid');
  assert.equal(unsafe.view.failureKind, 'invalid_config');

  const unreadable = finish(ready(), '', 1_100, 1, fixture('unreadable-config.json'));
  assert.equal(unreadable.view.chip, 'unknown');
  assert.equal(unreadable.view.failureKind, 'unreadable');
  assert.notEqual(unreadable.view.failureKind, 'missing_config');

  const schema = finish(ready(), fixture('schema-v2.json'), 1_100);
  assert.equal(schema.view.chip, 'malformed');
  assert.equal(schema.view.failureKind, 'malformed');
  assert.equal(schema.view.volumes.length, 0);

  const unknown = finish(ready(), '', 1_100, 1, fixture('unknown-error.json'));
  assert.equal(unknown.view.chip, 'unknown');
  assert.equal(unknown.view.failureKind, 'unknown');
  assert.equal(unknown.view.failureCode, 'catalog_failed');

  const garbage = finish(ready(), 'not json {', 1_100);
  assert.equal(garbage.view.chip, 'malformed');
  const empty = finish(ready(), '   ', 1_100, 0);
  assert.equal(empty.view.chip, 'malformed');
  const silent = finish(ready(), '', 1_100, 1, '');
  assert.equal(silent.view.chip, 'unknown');

  const mounted = finish(ready(), fixture('mounted.json'), 2_000);
  const broken = finish(api.noteTick(mounted.session, 62_000, false), fixture('schema-v2.json'), 63_000);
  assert.equal(broken.view.chip, 'present');
  assert.equal(broken.view.dimmed, true);
  assert.equal(broken.view.failureKind, 'malformed');
  assert.equal(broken.view.capturedAt, 2_000);
  assert.equal(broken.view.volumes[0].state, 'mounted');
});

test('an oversized status body is not parsed', () => {
  const mounted = finish(ready(), fixture('mounted.json'), 2_000);
  const huge = `${'{"schema_version":1,"state":"ready","chip":"present"}'}${ 'x'.repeat(api.OUTPUT_CAP)}`;
  const again = api.noteTick(mounted.session, 62_000, false);
  const capped = finish(again, huge, 63_000);
  assert.equal(capped.view.chip, 'present');
  assert.equal(capped.view.failureKind, 'output_capped');
  assert.equal(capped.view.capturedAt, 2_000);
  assert.equal(capped.view.transcript, '');

  const first = finish(ready(), 'y'.repeat(api.OUTPUT_CAP + 1), 1_100);
  assert.equal(first.view.chip, 'malformed');
  assert.equal(first.view.failureKind, 'output_capped');
  assert.equal(first.view.volumes.length, 0);
});

test('too many rows are malformed instead of a partial list', () => {
  const volume = {
    label: 'primary',
    volume_id: 'volume',
    path: '/vault/models',
    state: 'mounted',
    available_bytes: 1,
  };
  const tooMany = {
    schema_version: 1,
    state: 'ready',
    vault_id: '8397c2c3aaaaaaaaaaaaaaaaaaaaaaaa',
    volumes: Array.from({ length: 33 }, () => volume),
    preserved: [],
  };
  const done = finish(ready(), JSON.stringify(tooMany), 1_100);
  assert.equal(done.view.failureKind, 'malformed');
  assert.equal(done.view.volumes.length, 0);
});

test('a missing executable is not a disk failure', () => {
  const absent = api.configure(api.createSession(0), '   ', '/vault/config.toml', true, 10);
  assert.equal(absent.launch, null);
  assert.equal(absent.view.chip, 'absent');
  assert.equal(absent.view.failureKind, 'missing_executable');

  const noConfig = api.configure(api.createSession(0), 'modelprepper', '   ', true, 11);
  assert.equal(noConfig.launch, null);
  assert.equal(noConfig.view.chip, 'config');
  assert.equal(noConfig.view.failureKind, 'missing_config');

  const step = ready('missing-bin');
  const failed = api.noteResult(step.session, {
    generation: step.launch.generation,
    now: 1_050,
    exitCode: -1,
    stdout: fixture('mounted.json'),
    stderr: '',
    spawnError: 'failed_to_start',
  });
  assert.equal(failed.view.chip, 'absent');
  assert.equal(failed.view.failureKind, 'missing_executable');
  assert.equal(failed.view.volumes.length, 0);
});

test('timeout ignores the late result and does not loop', () => {
  const step = ready('modelprepper', '/vault/config.toml', 0);
  const early = api.noteTick(step.session, 3_999, false);
  assert.equal(early.launch, null);
  assert.equal(early.view.activity, 'checking');
  const timed = api.timeoutFlight(step.session, step.launch.generation, 4_000);
  assert.equal(timed.view.chip, 'timeout');
  assert.equal(timed.view.failureKind, 'timeout');
  assert.equal(timed.launch, null);
  const late = api.noteResult(timed.session, {
    generation: step.launch.generation,
    now: 4_500,
    exitCode: 0,
    stdout: fixture('mounted.json'),
    stderr: '',
    spawnError: '',
  });
  assert.equal(late.ignored, true);
  assert.equal(late.view.chip, 'timeout');
  assert.equal(late.view.volumes.length, 0);
  const retry = api.noteTick(timed.session, 4_001, false);
  assert.equal(retry.launch, null);
});

test('stale data stays the old presence and is not an offline disk', () => {
  const mounted = finish(ready(), fixture('mounted.json'), 10_000);
  const fresh = api.noteAge(mounted.session, 10_000 + api.STALE_MS - 1);
  assert.equal(fresh.view.phase, 'current');
  assert.equal(fresh.view.dimmed, false);
  const stale = api.noteAge(mounted.session, 10_000 + api.STALE_MS);
  assert.equal(stale.view.phase, 'stale');
  assert.equal(stale.view.dimmed, true);
  assert.equal(stale.view.chip, 'present');
  assert.equal(stale.view.failureKind, '');
  assert.equal(stale.view.volumes[0].state, 'mounted');
  assert.equal(stale.view.ageLabel ? stale.view.snapshotLine.includes('ago') : stale.view.snapshotLine.includes('m ago'), true);
  assert.equal(stale.launch, null);
});

test('changing the selected config drops the old snapshot and the old result', () => {
  const mounted = finish(ready(), fixture('mounted.json'), 2_000);
  const next = api.configure(mounted.session, 'modelprepper', '/other/config.toml', true, 3_000);
  assertStatusLaunch(next.launch, 'modelprepper', '/other/config.toml');
  assert.equal(next.view.volumes.length, 0);
  assert.equal(next.view.chip, 'unknown');
  assert.notEqual(next.launch.generation, mounted.session.generation);
  const late = api.noteResult(next.session, {
    generation: mounted.session.generation,
    now: 3_100,
    exitCode: 0,
    stdout: fixture('mounted.json'),
    stderr: '',
    spawnError: '',
  });
  assert.equal(late.ignored, true);
  assert.equal(late.view.volumes.length, 0);
  const accepted = api.noteResult(next.session, {
    generation: next.launch.generation,
    now: 3_200,
    exitCode: 0,
    stdout: fixture('offline.json'),
    stderr: '',
    spawnError: '',
  });
  assert.equal(accepted.ignored, false);
  assert.equal(accepted.view.chip, 'offline');
  assert.equal(accepted.view.configPath, '/other/config.toml');
  assert.equal(accepted.view.volumes[0].state, 'offline');
});

test('disable keeps the last snapshot and re-enable reads again', () => {
  const mounted = finish(ready(), fixture('mounted.json'), 2_000);
  const paused = api.configure(mounted.session, 'modelprepper', '/vault/config.toml', false, 9_000);
  assert.equal(paused.launch, null);
  assert.equal(paused.view.phase, 'paused');
  assert.equal(paused.view.dimmed, true);
  assert.equal(paused.view.chip, 'present');
  assert.equal(paused.view.capturedAt, 2_000);
  const late = api.noteResult(paused.session, {
    generation: mounted.session.generation,
    now: 9_500,
    exitCode: 1,
    stdout: '',
    stderr: fixture('writer-busy.json'),
    spawnError: '',
  });
  assert.equal(late.ignored, true);
  assert.equal(late.view.failureKind, '');
  assert.equal(late.view.capturedAt, 2_000);
  const aged = api.noteAge(paused.session, 2_000 + api.STALE_MS);
  assert.equal(aged.view.phase, 'paused');
  assert.equal(aged.view.chip, 'present');
  const resumed = api.configure(paused.session, 'modelprepper', '/vault/config.toml', true, 20_000);
  assertStatusLaunch(resumed.launch, 'modelprepper', '/vault/config.toml');
  assert.equal(resumed.view.capturedAt, 2_000);
  assert.notEqual(resumed.launch.generation, paused.session.generation);
});

test('one shared flight absorbs the other monitor and a forced refresh runs once', () => {
  api.resetShared(0);
  const first = api.sharedConfigure('modelprepper', '/vault/config.toml', true, 1_000);
  const second = api.sharedConfigure('modelprepper', '/vault/config.toml', true, 1_001);
  assertStatusLaunch(first.launch, 'modelprepper', '/vault/config.toml');
  assert.equal(second.launch, null);
  assert.equal(second.view.activity, 'checking');
  assert.equal(second.session.generation, first.session.generation);

  const otherTick = api.sharedTick(1_002, false);
  assert.equal(otherTick.launch, null);
  const done = api.sharedResult({
    generation: first.launch.generation,
    now: 1_100,
    exitCode: 0,
    stdout: fixture('mounted.json'),
    stderr: '',
    spawnError: '',
  });
  assert.equal(done.launch, null);
  assert.equal(done.view.chip, 'present');
  assert.equal(api.sharedAge(1_100).view.chip, 'present');
  assert.equal(api.sharedTick(1_000 + api.REFRESH_MS - 1, false).launch, null);

  const again = api.sharedTick(1_000 + api.REFRESH_MS, false);
  assertStatusLaunch(again.launch, 'modelprepper', '/vault/config.toml');
  const forced = api.sharedTick(1_000 + api.REFRESH_MS + 10, true);
  assert.equal(forced.launch, null);
  const follow = api.sharedResult({
    generation: again.launch.generation,
    now: 62_000,
    exitCode: 0,
    stdout: fixture('offline.json'),
    stderr: '',
    spawnError: '',
  });
  assertStatusLaunch(follow.launch, 'modelprepper', '/vault/config.toml');
  const quiet = api.sharedResult({
    generation: follow.launch.generation,
    now: 62_100,
    exitCode: 0,
    stdout: fixture('offline.json'),
    stderr: '',
    spawnError: '',
  });
  assert.equal(quiet.launch, null);
  assert.equal(quiet.view.chip, 'offline');
});

test('inspection waits for the current status read and uses the text argv', () => {
  api.resetShared(0);
  const first = api.sharedConfigure('modelprepper', '/vault/config.toml', true, 100);
  const waiting = api.sharedInspect(110);
  assert.equal(waiting.launch, null);
  assert.equal(waiting.view.activity, 'checking');
  const after = api.sharedResult({
    generation: first.launch.generation,
    now: 150,
    exitCode: 0,
    stdout: fixture('mounted.json'),
    stderr: '',
    spawnError: '',
  });
  assert.equal(after.launch.kind, 'inspect');
  assert.deepEqual(args(after.launch.argv), ['modelprepper', 'status', '--config', '/vault/config.toml']);
  assert.equal(after.launch.argv.includes('--json'), false);
  assert.equal(after.launch.argv.includes('--repo'), false);
  assert.equal(after.view.capturedAt, 150);
  const opened = api.sharedResult({
    generation: after.launch.generation,
    now: 200,
    exitCode: 0,
    stdout: fixture('mounted.json'),
    stderr: '',
    spawnError: '',
  });
  assert.equal(opened.launch, null);
  assert.equal(opened.view.chip, 'present');
  assert.equal(opened.view.failureKind, '');
  assert.equal(opened.view.capturedAt, 150);
  assert.equal(opened.view.transcript.includes('8397c2c3'), true);

  const direct = api.sharedInspect(250);
  assert.equal(direct.launch.kind, 'inspect');
  assert.deepEqual(args(direct.launch.argv), args(api.inspectArgv('modelprepper', '/vault/config.toml')));
  const text = api.sharedResult({
    generation: direct.launch.generation,
    now: 300,
    exitCode: 0,
    stdout: 'present\nprimary mounted\n',
    stderr: '',
    spawnError: '',
  });
  assert.equal(text.view.failureKind, '');
  assert.equal(text.view.chip, 'present');
  assert.equal(text.view.capturedAt, 150);
  assert.equal(text.view.transcript.includes('primary mounted'), true);
});

test('a capped status read still opens a waiting inspection', () => {
  api.resetShared(0);
  const first = api.sharedConfigure('modelprepper', '/vault/config.toml', true, 100);
  const ready = api.sharedResult({
    generation: first.launch.generation,
    now: 150,
    exitCode: 0,
    stdout: fixture('mounted.json'),
    stderr: '',
    spawnError: '',
  });
  assert.equal(ready.view.capturedAt, 150);
  const again = api.sharedTick(200, true);
  assertStatusLaunch(again.launch, 'modelprepper', '/vault/config.toml');
  const waiting = api.sharedInspect(210);
  assert.equal(waiting.launch, null);
  const capped = api.sharedCapped(again.launch.generation, 220);
  assert.equal(capped.launch.kind, 'inspect');
  assert.deepEqual(args(capped.launch.argv), ['modelprepper', 'status', '--config', '/vault/config.toml']);
  assert.equal(capped.view.failureKind, 'output_capped');
  assert.equal(capped.view.chip, 'present');
  assert.equal(capped.view.capturedAt, 150);
  assert.equal(capped.view.dimmed, true);
});

test('abandoning a destroyed widget ignores its late exit', () => {
  api.resetShared(0);
  const first = api.sharedConfigure('modelprepper', '/vault/config.toml', true, 100);
  const abandoned = api.sharedAbandon(first.launch.generation, 120);
  assert.equal(abandoned.launch, null);
  assert.equal(abandoned.view.activity, '');
  const late = api.sharedResult({
    generation: first.launch.generation,
    now: 130,
    exitCode: 0,
    stdout: fixture('mounted.json'),
    stderr: '',
    spawnError: '',
  });
  assert.equal(late.ignored, true);
  assert.equal(late.view.volumes.length, 0);
});

test('every launched command in a mixed session is status and nothing else', () => {
  api.resetShared(0);
  const launches = [];
  const remember = (step) => {
    if (step.launch) launches.push(step.launch);
    return step;
  };
  remember(api.sharedConfigure('modelprepper', '/vault/config.toml', true, 1));
  remember(api.sharedTick(2, false));
  remember(api.sharedTick(3, true));
  remember(api.sharedInspect(4));
  const status = launches[0];
  remember(api.sharedResult({
    generation: status.generation,
    now: 10,
    exitCode: 0,
    stdout: fixture('mounted.json'),
    stderr: '',
    spawnError: '',
  }));
  remember(api.sharedTimeout(launches.at(-1).generation, 20));
  remember(api.sharedCapped(launches.at(-1).generation, 30));
  assert.ok(launches.length >= 2);
  for (const launch of launches) {
    assert.equal(launch.argv[1], 'status');
    assert.equal(api.argvOk(launch.argv, launch.kind), true);
    assert.equal(launch.argv.includes('--repo'), false);
    assert.equal(launch.argv.includes('--plan'), false);
    for (const token of forbiddenTokens) assert.equal(launch.argv.includes(token), false);
  }
});

test('the plugin manifest is a single optional bar widget', () => {
  const manifest = JSON.parse(readFileSync(join(pluginDir, 'manifest.json'), 'utf8'));
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.id, 'io.github.blisspixel.modelprepper-presence');
  assert.equal(manifest.id.startsWith('omarchy.'), false);
  assert.equal(manifest.license, 'MIT');
  assert.deepEqual(manifest.kinds, ['bar-widget']);
  assert.deepEqual(manifest.entryPoints, { barWidget: 'BarWidget.qml' });
  assert.equal(manifest.barWidget.allowMultiple, false);
  assert.deepEqual(Object.keys(manifest.barWidget.defaults), ['config', 'executable']);
  assert.deepEqual(manifest.barWidget.schema.map((entry) => entry.key), ['config', 'executable']);
  assert.equal('omarchy' in manifest, false);
  assert.equal(lstatSync(join(pluginDir, 'BarWidget.qml')).isSymbolicLink(), false);
  assert.equal(lstatSync(join(pluginDir, 'Panel.qml')).isSymbolicLink(), false);
  assert.equal(lstatSync(join(pluginDir, 'Model.js')).isSymbolicLink(), false);
  const names = readdirSync(pluginDir);
  assert.equal(names.includes('BarWidget.qml'), true);
  assert.equal(names.includes('Panel.qml'), true);
  assert.equal(names.filter((name) => name.endsWith('.qml') || name.endsWith('.js') || name === 'manifest.json').every((name) => !lstatSync(join(pluginDir, name)).isSymbolicLink()), true);
});

test('the QML shell only displays the model and does not build a command', () => {
  const bar = readFileSync(join(pluginDir, 'BarWidget.qml'), 'utf8');
  const panel = readFileSync(join(pluginDir, 'Panel.qml'), 'utf8');
  const readme = readFileSync(join(pluginDir, 'README.md'), 'utf8');
  for (const file of [bar, panel]) {
    assert.equal(file.includes('io.github.blisspixel.modelprepper-presence'), true);
    for (const token of ['verify-local', 'adopt', 'resolve', 'propose', 'decide', 'startDetached', 'execDetached', 'sh']) {
      assert.equal(file.includes(`"${token}"`) || file.includes(`'${token}'`), false, token);
    }
  }
  assert.equal(panel.includes('Text.PlainText'), true);
  assert.equal(bar.includes('Text.StyledText'), false);
  assert.equal(panel.includes('Text.StyledText'), false);
  assert.equal(bar.includes('function open()'), true);
  assert.equal(bar.includes('function close()'), true);
  assert.equal(bar.includes('function toggle()'), true);
  assert.equal(bar.includes('function closeForPopoutSwitch()'), true);
  assert.equal(bar.includes('popoutSwitchClosing'), true);
  assert.equal(bar.includes('setting("executable", "modelprepper")'), true);
  assert.equal(bar.includes('setting("config", "")'), true);
  assert.equal(bar.includes('statusProcess.exec(launch.argv)'), true);
  assert.equal(bar.includes('Qt.callLater(function() { root.beginFlight(launch) })'), true);
  assert.equal(bar.includes('root.owedGeneration = launch.generation'), true);
  assert.equal(bar.includes('root.owedGeneration !== launch.generation'), true);
  assert.equal(bar.includes('function ownedGeneration()'), true);
  assert.equal(bar.includes('root.ownedGeneration()'), true);
  assert.equal(bar.includes('if (!root.sawStart || root.flightToken === 0) return'), true);
  assert.equal(bar.includes('function startLater(launch)'), true);
  assert.equal(bar.includes('if (root.owedGeneration === launch.generation) root.startLater(launch)'), true);
  assert.equal(bar.includes('exec(['), false);
  assert.equal(bar.includes('sharedTick'), true);
  assert.equal(bar.includes('sharedInspect'), true);
  assert.equal(bar.includes('sharedTimeout'), true);
  assert.equal(bar.includes('sharedCapped'), true);
  assert.equal(bar.includes('sharedAbandon'), true);
  assert.equal(bar.includes('dropExitHandler'), true);
  assert.equal(bar.includes('waitForEnd: false'), true);
  assert.equal(bar.includes('Model.outputCap()'), true);
  assert.equal(bar.includes('Model.refreshMs()'), true);
  assert.equal(panel.includes('onCloseRequested: root.close()'), true);
  assert.equal(panel.includes('modelData.state'), true);
  assert.equal(panel.includes('modelData.path'), true);
  assert.equal(panel.includes('modelData.space'), true);
  assert.equal(panel.includes('Quickshell.clipboardText = text'), true);
  assert.equal(panel.includes('Commons.Style'), true);
  assert.equal(panel.includes('Commons.Color'), true);
  assert.equal(readme.includes('marketplace'), true);
  assert.equal(readme.includes('were not run'), true);
  assert.equal(readme.includes('qmllint'), true);
  assert.equal(readme.toLowerCase().includes('all models safe'), false);
});
