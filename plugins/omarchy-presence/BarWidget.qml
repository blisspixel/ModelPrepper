pragma ComponentBehavior: Bound

import QtQuick
import Quickshell.Io
import qs.Ui as Ui
import "." as Presence
import "Model.js" as Model

// Optional presence chip. Disk state comes from Model.js. This file only
// forwards the panel lifecycle and starts the argv that library returns.
Ui.BarWidget {
  id: root
  moduleName: "io.github.blisspixel.modelprepper-presence"

  property var view: ({
    chip: "unknown",
    meaning: "Status is unknown.",
    dimmed: false,
    failureText: ""
  })
  property int flightToken: 0
  property int owedGeneration: 0
  property bool sawStart: false
  property var queuedLaunch: null
  property var exitHandler: null

  readonly property bool opened: details.opened
  readonly property bool popoutSwitchClosing: details.popoutSwitchClosing

  function open() { details.open() }
  function close() { details.close() }
  function toggle() { details.toggle() }
  function closeForPopoutSwitch() { details.closeForPopoutSwitch() }

  function follow(step) {
    if (!step) return
    root.view = step.view
    if (!step.launch) return
    // Start after the current exit signal finishes. Connecting the next
    // handler inside that signal can attribute the old exit to the new flight.
    // owedGeneration lets a newer launch supersede this one, and destruction
    // can abandon it if this call never reaches the process.
    var launch = step.launch
    root.owedGeneration = launch.generation
    root.startLater(launch)
  }

  function startLater(launch) {
    Qt.callLater(function() { root.beginFlight(launch) })
  }

  function syncSettings() {
    root.follow(Model.sharedConfigure(
      root.setting("executable", "modelprepper"),
      root.setting("config", ""),
      true,
      Date.now()
    ))
  }

  function requestRefresh() {
    root.follow(Model.sharedTick(Date.now(), true))
  }

  function requestInspect() {
    root.follow(Model.sharedInspect(Date.now()))
  }

  function beginFlight(launch) {
    if (!launch || root.owedGeneration !== launch.generation) return
    // A dying process and the next launch share one Process. Queue until it
    // has actually stopped so its exit cannot be recorded as the new flight.
    if (statusProcess.running) {
      root.queuedLaunch = launch
      return
    }
    root.queuedLaunch = null
    root.armFlight(launch)
  }

  function ownedGeneration() {
    if (root.queuedLaunch && root.queuedLaunch.generation) return root.queuedLaunch.generation
    if (root.owedGeneration !== 0) return root.owedGeneration
    return root.flightToken
  }

  function dropExitHandler() {
    var handler = root.exitHandler
    if (!handler) return
    root.exitHandler = null
    statusProcess.exited.disconnect(handler)
  }

  function armFlight(launch) {
    // A failed start never emits exited. Drop the previous handler or a later
    // successful exit would still invoke it.
    root.dropExitHandler()
    var generation = launch.generation
    root.flightToken = generation
    root.owedGeneration = 0
    root.sawStart = false
    var handler = function(exitCode) {
      root.dropExitHandler()
      if (root.flightToken !== generation) return
      root.flightToken = 0
      timeoutTimer.stop()
      root.follow(Model.sharedResult({
        generation: generation,
        now: Date.now(),
        exitCode: exitCode,
        stdout: statusStdout.text,
        stderr: statusStderr.text,
        spawnError: ""
      }))
    }
    root.exitHandler = handler
    statusProcess.exited.connect(handler)
    timeoutTimer.interval = launch.timeoutMs
    timeoutTimer.restart()
    statusProcess.exec(launch.argv)
  }

  function failStart() {
    var generation = root.flightToken
    if (generation === 0 || root.sawStart) return
    root.dropExitHandler()
    root.flightToken = 0
    timeoutTimer.stop()
    root.follow(Model.sharedResult({
      generation: generation,
      now: Date.now(),
      exitCode: -1,
      stdout: "",
      stderr: "",
      spawnError: "failed_to_start"
    }))
  }

  function noteOutput(text) {
    // Ignore buffers from a process that has not started this flight. A dying
    // read can still emit after the next exec, and that must not cap the new one.
    if (!root.sawStart || root.flightToken === 0) return
    if (text.length <= Model.outputCap()) return
    root.giveUp(root.flightToken, "cap")
  }

  function giveUp(generation, which) {
    if (root.flightToken !== generation) return
    root.dropExitHandler()
    root.flightToken = 0
    timeoutTimer.stop()
    if (statusProcess.running) statusProcess.running = false
    var step = which === "cap"
      ? Model.sharedCapped(generation, Date.now())
      : Model.sharedTimeout(generation, Date.now())
    root.follow(step)
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onSettingsChanged: root.syncSettings()

  Component.onCompleted: root.syncSettings()
  Component.onDestruction: {
    var generation = root.ownedGeneration()
    root.dropExitHandler()
    root.flightToken = 0
    root.owedGeneration = 0
    root.queuedLaunch = null
    if (generation !== 0) Model.sharedAbandon(generation, Date.now())
  }

  Presence.Panel {
    id: details
    visible: false
    bar: root.bar
    settings: root.settings
    anchorItem: button
    hostWidget: root
  }

  Timer {
    interval: 1000
    running: true
    repeat: true
    onTriggered: root.view = Model.sharedAge(Date.now()).view
  }

  Timer {
    interval: Model.refreshMs()
    running: true
    repeat: true
    onTriggered: root.follow(Model.sharedTick(Date.now(), false))
  }

  Timer {
    id: timeoutTimer
    repeat: false
    onTriggered: root.giveUp(root.flightToken, "timeout")
  }

  Process {
    id: statusProcess
    stdinEnabled: false
    stdout: StdioCollector {
      id: statusStdout
      waitForEnd: false
      onDataChanged: root.noteOutput(statusStdout.text)
    }
    stderr: StdioCollector {
      id: statusStderr
      waitForEnd: false
      onDataChanged: root.noteOutput(statusStderr.text)
    }
    onStarted: root.sawStart = true
    onRunningChanged: {
      if (statusProcess.running) return
      if (root.queuedLaunch) {
        const launch = root.queuedLaunch
        root.queuedLaunch = null
        // A newer launch may already own the generation. Starting the stale
        // one would let its exit stand in for the read the model is waiting on.
        if (root.owedGeneration === launch.generation) root.startLater(launch)
        return
      }
      root.failStart()
    }
  }

  Ui.WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.view.chip
    opacity: root.view.dimmed ? 0.45 : 1
    tooltipText: root.view.failureText
      ? root.view.meaning + " " + root.view.failureText
      : root.view.meaning
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.LeftButton) root.toggle()
    }
  }
}
