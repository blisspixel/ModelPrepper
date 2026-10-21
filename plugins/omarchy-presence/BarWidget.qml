pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import qs.Ui
import "Model.js" as Model

// Optional presence chip. Disk state comes from Model.js. This file only
// forwards the panel lifecycle and starts the argv that library returns.
BarWidget {
  id: root
  moduleName: "io.github.blisspixel.modelprepper-presence"

  property var view: ({
    chip: "unknown",
    meaning: "Status is unknown.",
    dimmed: false,
    failureText: ""
  })
  property int flightToken: 0
  property bool sawStart: false
  property var queuedLaunch: null
  property var exitHandler: null

  readonly property bool opened: panelLoader.item
    ? panelLoader.item.opened === true
    : false
  readonly property bool popoutSwitchClosing: panelLoader.item
    ? panelLoader.item.popoutSwitchClosing === true
    : false

  function open() {
    if (panelLoader.item) panelLoader.item.open()
  }

  function close() {
    if (panelLoader.item) panelLoader.item.close()
  }

  function toggle() {
    if (panelLoader.item) panelLoader.item.toggle()
  }

  function closeForPopoutSwitch() {
    if (panelLoader.item) panelLoader.item.closeForPopoutSwitch()
  }

  function injectPanel() {
    var target = panelLoader.item
    if (!target) return
    if ("bar" in target) target.bar = root.bar
    if ("settings" in target) target.settings = root.settings
    if ("anchorItem" in target) target.anchorItem = button
    if ("hostWidget" in target) target.hostWidget = root
  }

  function follow(step) {
    if (!step) return
    root.view = step.view
    if (step.launch) root.beginFlight(step.launch)
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
    // A dying process and the next launch share one Process. Queue until it
    // has actually stopped so its exit cannot be recorded as the new flight.
    if (statusProcess.running) {
      root.queuedLaunch = launch
      return
    }
    root.queuedLaunch = null
    root.armFlight(launch)
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

  onBarChanged: injectPanel()
  onSettingsChanged: {
    root.injectPanel()
    root.syncSettings()
  }

  Component.onCompleted: root.syncSettings()
  Component.onDestruction: {
    var generation = root.flightToken
    root.dropExitHandler()
    root.flightToken = 0
    if (generation !== 0) Model.sharedAbandon(generation, Date.now())
  }

  Loader {
    id: panelLoader
    active: true
    source: Qt.resolvedUrl("Panel.qml")
    visible: false
    onLoaded: {
      root.injectPanel()
      Qt.callLater(root.injectPanel)
    }
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
      onDataChanged: {
        if (statusStdout.text.length <= Model.outputCap()) return
        root.giveUp(root.flightToken, "cap")
      }
    }
    stderr: StdioCollector {
      id: statusStderr
      waitForEnd: false
      onDataChanged: {
        if (statusStderr.text.length <= Model.outputCap()) return
        root.giveUp(root.flightToken, "cap")
      }
    }
    onStarted: root.sawStart = true
    onRunningChanged: {
      if (statusProcess.running) return
      if (root.queuedLaunch) {
        var launch = root.queuedLaunch
        root.queuedLaunch = null
        root.armFlight(launch)
        return
      }
      root.failStart()
    }
  }

  WidgetButton {
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
