.pragma library

// One projection of `modelprepper status` for every monitor.
// The QML files import this library. Node tests strip the first line and
// evaluate the same source. Do not keep a second copy of these rules.

var PROJECTION_VERSION = 1
var OUTPUT_CAP = 262144
var TRANSCRIPT_CAP = 2000
var MESSAGE_CAP = 240
var TIMEOUT_MS = 4000
var REFRESH_MS = 60000
var STALE_MS = 120000
var MAX_VOLUMES = 32
var MAX_PRESERVED = 64
var MAX_SAFE_INTEGER = 9007199254740991

function outputCap() { return OUTPUT_CAP }
function timeoutMs() { return TIMEOUT_MS }
function refreshMs() { return REFRESH_MS }
function staleMs() { return STALE_MS }
function projectionVersion() { return PROJECTION_VERSION }

function statusArgv(executable, configPath) {
    return [String(executable), "status", "--json", "--config", String(configPath)]
}

function inspectArgv(executable, configPath) {
    return [String(executable), "status", "--config", String(configPath)]
}

function argvOk(argv, kind) {
    if (!argv || typeof argv.length !== "number") return false
    var expected = kind === "inspect" ? 4 : 5
    if (argv.length !== expected) return false
    if (!argv[0] || argv[1] !== "status") return false
    if (kind === "inspect") {
        if (argv[2] !== "--config" || !argv[3]) return false
    } else if (argv[2] !== "--json" || argv[3] !== "--config" || !argv[4]) {
        return false
    }
    var index
    for (index = 0; index < argv.length; index++) {
        if (String(argv[index]).indexOf("\0") !== -1) return false
    }
    return true
}

function copyText(argv) {
    if (!argv) return ""
    return JSON.stringify(argv)
}

function trim(text) {
    return String(text).replace(/^\s+|\s+$/g, "")
}

function hasNul(text) {
    return String(text).indexOf("\0") !== -1
}

function sanitize(text) {
    var raw = String(text)
    var out = ""
    var index
    for (index = 0; index < raw.length; index++) {
        var code = raw.charCodeAt(index)
        out += (code < 32 || code === 127) ? " " : raw.charAt(index)
    }
    return out
}

function clipMessage(text) {
    var clean = sanitize(text)
    if (clean.length <= MESSAGE_CAP) return clean
    return clean.substring(0, MESSAGE_CAP) + "..."
}

function clipTranscript(text) {
    var clean = sanitize(text)
    if (clean.length <= TRANSCRIPT_CAP) return clean
    return clean.substring(0, TRANSCRIPT_CAP) + "..."
}

function pad(value) {
    return value < 10 ? "0" + value : String(value)
}

function isoUtc(ms) {
    var date = new Date(ms)
    return date.getUTCFullYear() + "-" + pad(date.getUTCMonth() + 1) + "-" + pad(date.getUTCDate())
        + " " + pad(date.getUTCHours()) + ":" + pad(date.getUTCMinutes()) + " UTC"
}

function ageLabel(ms) {
    if (ms < 5000) return "just now"
    if (ms < 60000) return Math.floor(ms / 1000) + "s ago"
    if (ms < 3600000) return Math.floor(ms / 60000) + "m ago"
    if (ms < 86400000) return Math.floor(ms / 3600000) + "h ago"
    return Math.floor(ms / 86400000) + "d ago"
}

function shortId(id) {
    var text = sanitize(id)
    if (!text) return "unknown"
    return text.length <= 8 ? text : text.substring(0, 8)
}

function isSafeInteger(value) {
    return typeof value === "number" && isFinite(value) && Math.floor(value) === value
        && value >= -MAX_SAFE_INTEGER && value <= MAX_SAFE_INTEGER
}

function scaled(bytes, base, units) {
    if (bytes < base) return bytes + " " + units[0]
    var value = bytes
    var index = 0
    while (value >= base && index + 1 < units.length) {
        value = value / base
        index = index + 1
    }
    return value.toFixed(1) + " " + units[index]
}

function humanPair(bytes) {
    return scaled(bytes, 1000, ["B", "KB", "MB", "GB", "TB"])
        + " (" + scaled(bytes, 1024, ["B", "KiB", "MiB", "GiB", "TiB"]) + ")"
}

function spaceLabel(bytes) {
    if (!isSafeInteger(bytes) || bytes < 0) return "space not measured"
    return humanPair(bytes) + " free"
}

function meaning(chip) {
    if (chip === "present") return "Every configured disk is mounted."
    if (chip === "offline") return "A configured disk is offline."
    if (chip === "mismatch") return "A disk identity does not match this vault."
    if (chip === "missing") return "A disk is present but its identity marker is missing."
    if (chip === "empty") return "This vault has no configured disks."
    if (chip === "blocked") return "Catalog setup is not finished."
    if (chip === "busy") return "The vault writer is busy."
    if (chip === "config") return "No config file is selected."
    if (chip === "invalid") return "The selected config is not usable."
    if (chip === "malformed") return "Status output was malformed."
    if (chip === "timeout") return "Status did not finish in time."
    if (chip === "absent") return "The modelprepper executable is missing."
    return "Status is unknown."
}

function makeFailure(kind, chip, message, code) {
    return {
        kind: kind,
        chip: chip,
        message: clipMessage(message || kind),
        code: code ? String(code) : ""
    }
}

function isMissing(message) {
    var lower = String(message).toLowerCase()
    return lower.indexOf("os error 2") !== -1
        || lower.indexOf("not found") !== -1
        || lower.indexOf("no such file") !== -1
        || lower.indexOf("cannot find the file") !== -1
}

function classifyError(error) {
    if (!error || typeof error !== "object" || Array.isArray(error)) {
        return makeFailure("malformed", "malformed", "status error was not an object", "")
    }
    var code = typeof error.code === "string" ? error.code : ""
    var message = typeof error.message === "string" && error.message
        ? clipMessage(error.message)
        : (code || "status failed")
    if (code === "writer_busy") return makeFailure("writer_busy", "busy", message, code)
    if (code === "invalid_config" || code === "unsafe_vault_path" || code === "volume_configuration_changed") {
        return makeFailure("invalid_config", "invalid", message, code)
    }
    if (code === "input_read_failed") {
        if (isMissing(message)) return makeFailure("missing_config", "config", message, code)
        return makeFailure("unreadable", "unknown", message, code)
    }
    return makeFailure("unknown", "unknown", message, code)
}

function presenceOf(volumes, vaultState) {
    if (volumes.length === 0) return "empty"
    var worst = -1
    var chip = "present"
    var index
    for (index = 0; index < volumes.length; index++) {
        var state = volumes[index].state
        var rank = 0
        var word = "present"
        if (state === "identity_mismatch") {
            rank = 4
            word = "mismatch"
        } else if (state === "identity_missing") {
            rank = 3
            word = "missing"
        } else if (state === "offline") {
            rank = 2
            word = "offline"
        } else if (state === "mounted") {
            rank = 0
            word = "present"
        } else {
            rank = 1
            word = "unknown"
        }
        if (rank > worst) {
            worst = rank
            chip = word
        }
    }
    if (chip === "present" && vaultState !== "ready") return "blocked"
    return chip
}

function stringField(value) {
    return typeof value === "string" ? value : null
}

function projectSnapshot(value) {
    if (!value || typeof value !== "object" || Array.isArray(value)) {
        return { failure: makeFailure("malformed", "malformed", "status output was not an object", "") }
    }
    if (value.schema_version !== 1) {
        return { failure: makeFailure("malformed", "malformed", "status schema_version must be 1", "") }
    }
    var vaultState = stringField(value.state)
    var vaultId = stringField(value.vault_id)
    if (vaultState === null || vaultId === null || !Array.isArray(value.volumes) || !Array.isArray(value.preserved)) {
        return { failure: makeFailure("malformed", "malformed", "status is missing vault, volumes, or custody rows", "") }
    }
    if (value.volumes.length > MAX_VOLUMES || value.preserved.length > MAX_PRESERVED) {
        return { failure: makeFailure("malformed", "malformed", "status listed more rows than the panel can show", "") }
    }
    var volumes = []
    var index
    for (index = 0; index < value.volumes.length; index++) {
        var volume = value.volumes[index]
        if (!volume || typeof volume !== "object" || Array.isArray(volume)) {
            return { failure: makeFailure("malformed", "malformed", "a volume entry was not an object", "") }
        }
        var label = stringField(volume.label)
        var volumeId = stringField(volume.volume_id)
        var path = stringField(volume.path)
        var state = stringField(volume.state)
        if (label === null || volumeId === null || path === null || state === null) {
            return { failure: makeFailure("malformed", "malformed", "a volume entry was missing a field", "") }
        }
        volumes.push({
            label: sanitize(label),
            state: sanitize(state),
            space: spaceLabel(volume.available_bytes),
            path: sanitize(path)
        })
    }
    var preserved = []
    for (index = 0; index < value.preserved.length; index++) {
        var row = value.preserved[index]
        if (!row || typeof row !== "object" || Array.isArray(row)) {
            return { failure: makeFailure("malformed", "malformed", "a custody row was not an object", "") }
        }
        var repo = stringField(row.repo_id)
        var revision = stringField(row.resolved_revision)
        var provenance = stringField(row.provenance)
        var volumeLabel = stringField(row.volume_label)
        if (repo === null || revision === null || provenance === null || volumeLabel === null) {
            return { failure: makeFailure("malformed", "malformed", "a custody row was missing a field", "") }
        }
        preserved.push({
            repo: sanitize(repo),
            revision: shortId(revision),
            provenance: sanitize(provenance),
            volume: sanitize(volumeLabel)
        })
    }
    var cleanState = sanitize(vaultState)
    return {
        snapshot: {
            capturedAt: 0,
            presence: presenceOf(volumes, cleanState),
            vaultState: cleanState,
            vaultId: shortId(vaultId),
            volumes: volumes,
            preserved: preserved
        }
    }
}

function parseBody(text, fromFailure) {
    var value
    try {
        value = JSON.parse(text)
    } catch (error) {
        return { failure: makeFailure("malformed", "malformed", "status output was not JSON", ""), raw: text }
    }
    if (!value || typeof value !== "object" || Array.isArray(value)) {
        return { failure: makeFailure("malformed", "malformed", "status output was not an object", ""), raw: text }
    }
    if (value.error) return { failure: classifyError(value.error), raw: text }
    if (fromFailure) {
        return { failure: makeFailure("unknown", "unknown", "status failed without an error object", ""), raw: text }
    }
    var projected = projectSnapshot(value)
    if (projected.failure) return { failure: projected.failure, raw: text }
    return { snapshot: projected.snapshot, raw: text }
}

function parseStreams(stdout, stderr, exitCode) {
    var out = trim(stdout)
    var err = trim(stderr)
    if (exitCode !== 0) {
        var errorBody = err || out
        if (!errorBody) {
            return { failure: makeFailure("unknown", "unknown", "status exited without output", String(exitCode)), raw: "" }
        }
        return parseBody(errorBody, true)
    }
    var body = out || err
    if (!body) return { failure: makeFailure("malformed", "malformed", "status returned an empty document", ""), raw: "" }
    return parseBody(body, false)
}

function createSession(now) {
    return {
        generation: 1,
        enabled: true,
        configPath: "",
        executable: "",
        inFlight: null,
        pendingPoll: false,
        pendingForce: false,
        pendingInspect: false,
        lastAttemptAt: -1,
        lastGood: null,
        failure: null,
        transcript: "",
        paused: false
    }
}

function cloneSession(session) {
    return {
        generation: session.generation,
        enabled: session.enabled,
        configPath: session.configPath,
        executable: session.executable,
        inFlight: session.inFlight ? {
            generation: session.inFlight.generation,
            startedAt: session.inFlight.startedAt,
            kind: session.inFlight.kind
        } : null,
        pendingPoll: session.pendingPoll,
        pendingForce: session.pendingForce,
        pendingInspect: session.pendingInspect,
        lastAttemptAt: session.lastAttemptAt,
        lastGood: session.lastGood,
        failure: session.failure,
        transcript: session.transcript,
        paused: session.paused
    }
}

function projectView(session, now) {
    var snapshot = session.lastGood
    var failure = session.failure
    var age = snapshot ? Math.max(0, now - snapshot.capturedAt) : 0
    var stale = !!(snapshot && age >= STALE_MS)
    var chip = snapshot ? snapshot.presence : (failure ? failure.chip : "unknown")
    var phase = "empty"
    if (!session.enabled) phase = "paused"
    else if (failure) phase = "failed"
    else if (stale) phase = "stale"
    else if (snapshot) phase = "current"
    var argv = []
    var copied = ""
    if (session.executable && session.configPath && !hasNul(session.executable) && !hasNul(session.configPath)) {
        argv = inspectArgv(session.executable, session.configPath)
        copied = copyText(argv)
    }
    return {
        projectionVersion: PROJECTION_VERSION,
        chip: chip,
        meaning: meaning(chip),
        dimmed: !session.enabled || !!failure || stale,
        phase: phase,
        activity: session.inFlight ? "checking" : "",
        failureKind: failure ? failure.kind : "",
        failureCode: failure ? failure.code : "",
        failureText: failure ? failure.message : "",
        snapshotLine: snapshot
            ? "snapshot  " + isoUtc(snapshot.capturedAt) + "  " + ageLabel(age)
            : "snapshot  none",
        capturedAt: snapshot ? snapshot.capturedAt : 0,
        ageMs: age,
        configLine: "config  " + (session.configPath ? sanitize(session.configPath) : "none"),
        configPath: sanitize(session.configPath),
        vaultState: snapshot ? snapshot.vaultState : "",
        vaultId: snapshot ? snapshot.vaultId : "",
        volumes: snapshot ? snapshot.volumes : [],
        preserved: snapshot ? snapshot.preserved : [],
        preservedNote: "Historical catalog provenance. Not a fresh integrity check.",
        transcript: session.transcript,
        copyText: copied,
        inspectArgv: argv
    }
}

function finish(session, now, launch, ignored) {
    return {
        session: session,
        launch: launch,
        view: projectView(session, now),
        ignored: !!ignored
    }
}

function blocker(session) {
    if (hasNul(session.executable) || hasNul(session.configPath)) {
        return makeFailure("invalid_config", "invalid", "the config or executable contains a null character", "")
    }
    if (!session.configPath) return makeFailure("missing_config", "config", "choose a config file", "")
    if (!session.executable) return makeFailure("missing_executable", "absent", "set the modelprepper executable", "")
    return null
}

function start(session, now, kind) {
    var problem = blocker(session)
    if (problem) {
        session.failure = problem
        session.pendingInspect = false
        session.pendingForce = false
        session.pendingPoll = false
        return finish(session, now, null, false)
    }
    var argv = kind === "inspect"
        ? inspectArgv(session.executable, session.configPath)
        : statusArgv(session.executable, session.configPath)
    if (!argvOk(argv, kind)) {
        session.failure = makeFailure("unknown", "unknown", "refused to launch an unexpected command", "")
        return finish(session, now, null, false)
    }
    session.inFlight = { generation: session.generation, startedAt: now, kind: kind }
    session.lastAttemptAt = now
    session.pendingPoll = false
    if (kind === "inspect") session.pendingInspect = false
    else session.pendingForce = false
    return finish(session, now, {
        argv: argv,
        generation: session.generation,
        timeoutMs: TIMEOUT_MS,
        outputCap: OUTPUT_CAP,
        kind: kind
    }, false)
}

function maybeLaunch(session, now, force) {
    if (session.inFlight) {
        if (now - session.inFlight.startedAt >= TIMEOUT_MS) return timeoutFlight(session, session.inFlight.generation, now)
        session = cloneSession(session)
        if (force) session.pendingForce = true
        else session.pendingPoll = true
        return finish(session, now, null, false)
    }
    if (!session.enabled) return finish(session, now, null, false)
    session = cloneSession(session)
    var problem = blocker(session)
    if (problem) {
        session.failure = problem
        session.pendingInspect = false
        session.pendingForce = false
        session.pendingPoll = false
        return finish(session, now, null, false)
    }
    var wantInspect = session.pendingInspect
    var wantForce = !!force || session.pendingForce
    var due = session.lastAttemptAt < 0 || now - session.lastAttemptAt >= REFRESH_MS
    if (!wantInspect && !wantForce && !due && !session.pendingPoll) return finish(session, now, null, false)
    if (wantInspect) return start(session, now, "inspect")
    return start(session, now, "status")
}

function noteTick(session, now, force) {
    if (session.inFlight && now - session.inFlight.startedAt >= TIMEOUT_MS) {
        return timeoutFlight(session, session.inFlight.generation, now)
    }
    if (session.inFlight) {
        session = cloneSession(session)
        if (force) session.pendingForce = true
        else session.pendingPoll = true
        return finish(session, now, null, false)
    }
    return maybeLaunch(session, now, !!force)
}

function configure(session, executable, configPath, enabled, now) {
    var nextExecutable = executable === null || executable === undefined ? "" : String(executable)
    var nextConfig = configPath === null || configPath === undefined ? "" : String(configPath)
    var nul = hasNul(nextExecutable) || hasNul(nextConfig)
    if (!nul) {
        nextExecutable = trim(nextExecutable)
        nextConfig = trim(nextConfig)
    }
    var nextEnabled = !!enabled
    if (session.executable === nextExecutable && session.configPath === nextConfig && session.enabled === nextEnabled && !nul) {
        return finish(session, now, null, false)
    }
    session = cloneSession(session)
    var configChanged = session.configPath !== nextConfig
    session.executable = nul ? "" : nextExecutable
    session.configPath = nul ? "" : nextConfig
    session.enabled = nextEnabled
    session.paused = !nextEnabled
    session.generation = session.generation + 1
    session.inFlight = null
    session.pendingPoll = false
    session.pendingForce = false
    session.pendingInspect = false
    if (configChanged || nul) {
        session.lastGood = null
        session.failure = null
        session.transcript = ""
        session.lastAttemptAt = -1
    }
    if (nul) {
        session.failure = makeFailure("invalid_config", "invalid", "the config or executable contains a null character", "")
        return finish(session, now, null, false)
    }
    if (!nextEnabled) return finish(session, now, null, false)
    return maybeLaunch(session, now, true)
}

function applyOutput(session, event) {
    if (event.spawnError) {
        var started = event.spawnError === "failed_to_start"
        session.failure = makeFailure(
            started ? "missing_executable" : "unknown",
            started ? "absent" : "unknown",
            started ? "the executable did not start" : "the executable failed before status could run",
            event.spawnError
        )
        return session
    }
    var stdout = typeof event.stdout === "string" ? event.stdout : ""
    var stderr = typeof event.stderr === "string" ? event.stderr : ""
    if (stdout.length > OUTPUT_CAP || stderr.length > OUTPUT_CAP) {
        session.failure = makeFailure("output_capped", "malformed", "status output exceeded the cap", "")
        session.transcript = ""
        return session
    }
    var exitCode = typeof event.exitCode === "number" ? event.exitCode : 1
    var parsed = parseStreams(stdout, stderr, exitCode)
    if (parsed.failure) {
        session.failure = parsed.failure
        session.transcript = clipTranscript(parsed.raw || "")
        return session
    }
    parsed.snapshot.capturedAt = event.now
    session.lastGood = parsed.snapshot
    session.failure = null
    session.transcript = clipTranscript(parsed.raw || "")
    return session
}

function streamText(value) {
    return typeof value === "string" ? value : ""
}

function withinCap(event) {
    return streamText(event.stdout).length <= OUTPUT_CAP && streamText(event.stderr).length <= OUTPUT_CAP
}

// Open inspection is the canonical text command. Its stdout is a pipe, so the
// CLI may return JSON, and a terminal would return text. Either result is a
// transcript. It is not a new disk projection, and text must not look malformed.
function inspectFinished(kind, event) {
    return kind === "inspect"
        && event
        && !event.spawnError
        && typeof event.exitCode === "number"
        && event.exitCode === 0
        && withinCap(event)
}

function noteResult(session, event) {
    var now = event.now
    if (!session.inFlight || session.inFlight.generation !== event.generation) {
        return finish(session, now, null, true)
    }
    var kind = session.inFlight.kind
    session = cloneSession(session)
    session.inFlight = null
    if (inspectFinished(kind, event)) {
        session.failure = null
        session.transcript = clipTranscript(trim(streamText(event.stdout) || streamText(event.stderr)))
    } else {
        session = applyOutput(session, event)
    }
    if (!session.enabled) {
        session.pendingPoll = false
        session.pendingForce = false
        session.pendingInspect = false
        return finish(session, now, null, false)
    }
    if (session.pendingInspect) return start(session, now, "inspect")
    if (session.pendingForce) {
        session.pendingPoll = false
        return start(session, now, "status")
    }
    session.pendingPoll = false
    return finish(session, now, null, false)
}

function timeoutFlight(session, generation, now) {
    if (!session.inFlight || session.inFlight.generation !== generation) {
        return finish(session, now, null, true)
    }
    session = cloneSession(session)
    session.inFlight = null
    session.generation = session.generation + 1
    session.failure = makeFailure("timeout", "timeout", "status did not finish before the deadline", "")
    session.pendingPoll = false
    if (session.pendingInspect && session.enabled) return start(session, now, "inspect")
    if (session.pendingForce && session.enabled) return start(session, now, "status")
    return finish(session, now, null, false)
}

function cappedFlight(session, generation, now) {
    if (!session.inFlight || session.inFlight.generation !== generation) {
        return finish(session, now, null, true)
    }
    session = cloneSession(session)
    session.inFlight = null
    session.generation = session.generation + 1
    session.failure = makeFailure("output_capped", "malformed", "status output exceeded the cap", "")
    session.transcript = ""
    session.pendingPoll = false
    if (session.pendingInspect && session.enabled) return start(session, now, "inspect")
    if (session.pendingForce && session.enabled) return start(session, now, "status")
    return finish(session, now, null, false)
}

function abandonFlight(session, generation, now) {
    if (!session.inFlight || session.inFlight.generation !== generation) {
        return finish(session, now, null, true)
    }
    session = cloneSession(session)
    session.inFlight = null
    session.generation = session.generation + 1
    session.pendingPoll = false
    session.pendingForce = false
    session.pendingInspect = false
    return finish(session, now, null, false)
}

function requestInspect(session, now) {
    if (session.inFlight) {
        session = cloneSession(session)
        session.pendingInspect = true
        return finish(session, now, null, false)
    }
    session = cloneSession(session)
    session.pendingInspect = true
    return maybeLaunch(session, now, false)
}

function noteAge(session, now) {
    return finish(session, now, null, false)
}

var shared = createSession(0)

function sharedConfigure(executable, configPath, enabled, now) {
    var step = configure(shared, executable, configPath, enabled, now)
    shared = step.session
    return step
}

function sharedTick(now, force) {
    var step = noteTick(shared, now, force)
    shared = step.session
    return step
}

function sharedInspect(now) {
    var step = requestInspect(shared, now)
    shared = step.session
    return step
}

function sharedResult(event) {
    var step = noteResult(shared, event)
    shared = step.session
    return step
}

function sharedTimeout(generation, now) {
    var step = timeoutFlight(shared, generation, now)
    shared = step.session
    return step
}

function sharedCapped(generation, now) {
    var step = cappedFlight(shared, generation, now)
    shared = step.session
    return step
}

function sharedAbandon(generation, now) {
    var step = abandonFlight(shared, generation, now)
    shared = step.session
    return step
}

function sharedAge(now) {
    return noteAge(shared, now)
}

function resetShared(now) {
    shared = createSession(now || 0)
    return sharedAge(now || 0)
}

var api = {
    PROJECTION_VERSION: PROJECTION_VERSION,
    OUTPUT_CAP: OUTPUT_CAP,
    TIMEOUT_MS: TIMEOUT_MS,
    REFRESH_MS: REFRESH_MS,
    STALE_MS: STALE_MS,
    outputCap: outputCap,
    timeoutMs: timeoutMs,
    refreshMs: refreshMs,
    staleMs: staleMs,
    projectionVersion: projectionVersion,
    statusArgv: statusArgv,
    inspectArgv: inspectArgv,
    argvOk: argvOk,
    copyText: copyText,
    humanPair: humanPair,
    spaceLabel: spaceLabel,
    createSession: createSession,
    configure: configure,
    noteTick: noteTick,
    noteResult: noteResult,
    noteAge: noteAge,
    timeoutFlight: timeoutFlight,
    cappedFlight: cappedFlight,
    abandonFlight: abandonFlight,
    requestInspect: requestInspect,
    resetShared: resetShared,
    sharedConfigure: sharedConfigure,
    sharedTick: sharedTick,
    sharedInspect: sharedInspect,
    sharedResult: sharedResult,
    sharedTimeout: sharedTimeout,
    sharedCapped: sharedCapped,
    sharedAbandon: sharedAbandon,
    sharedAge: sharedAge
}
