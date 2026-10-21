pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import qs.Commons as Commons
import qs.Ui
import "Model.js" as Model

// Nested details for the presence chip. Escape closes through PanelKeyCatcher.
// Qt 6.12 adds QtQuick.Color, which shadows an unqualified Commons color name,
// so theme colors and spacing stay on the Commons qualifier.
Panel {
  id: root
  moduleName: "io.github.blisspixel.modelprepper-presence"
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  property string notice: ""
  property var view: ({
    chip: "unknown",
    meaning: "Status is unknown.",
    dimmed: false,
    phase: "empty",
    activity: "",
    failureKind: "",
    failureCode: "",
    failureText: "",
    snapshotLine: "snapshot  none",
    configLine: "config  none",
    vaultState: "",
    vaultId: "",
    volumes: [],
    preserved: [],
    preservedNote: "Historical catalog provenance. Not a fresh integrity check.",
    transcript: "",
    copyText: ""
  })

  readonly property color ink: root.bar ? root.barForeground : Commons.Color.foreground
  readonly property string face: root.bar ? root.bar.fontFamily : Commons.Style.font.family

  function open() {
    root.refreshView()
    root.controller.show()
  }

  function close() {
    root.controller.hide()
  }

  function toggle() {
    if (root.opened) root.close()
    else root.open()
  }

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.hostWidget || root, direction)
    return false
  }

  function refreshView() {
    var step = Model.sharedAge(Date.now())
    if (step && step.view) root.view = step.view
  }

  function run(action) {
    if (action === "copy") {
      root.copyArguments()
      return
    }
    if (!root.hostWidget) return
    if (action === "refresh" && root.hostWidget.requestRefresh) root.hostWidget.requestRefresh()
    else if (action === "inspect" && root.hostWidget.requestInspect) root.hostWidget.requestInspect()
    root.refreshView()
  }

  function copyArguments() {
    var text = root.view.copyText
    if (!text) {
      root.notice = "Nothing to copy until a config and an executable are set."
      return
    }
    Quickshell.clipboardText = text
    root.notice = "Copied the argument list. It is not a shell command."
  }

  Timer {
    interval: 1000
    running: root.opened
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refreshView()
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.hostWidget || root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Commons.Style.space(420))
    contentHeight: panel.fittedContentHeight(content.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Column {
        id: content
        width: parent.width
        spacing: Commons.Style.space(8)

        Text {
          width: parent.width
          text: root.view.chip
          textFormat: Text.PlainText
          color: root.ink
          font.family: root.face
          font.pixelSize: Commons.Style.font.subtitle
          font.bold: true
          opacity: root.view.dimmed ? 0.45 : 1
          wrapMode: Text.WordWrap
        }

        Text {
          width: parent.width
          text: root.view.meaning
          textFormat: Text.PlainText
          color: root.ink
          font.family: root.face
          font.pixelSize: Commons.Style.font.body
          wrapMode: Text.WordWrap
        }

        Text {
          width: parent.width
          visible: root.view.activity !== ""
          text: root.view.activity
          textFormat: Text.PlainText
          color: root.ink
          font.family: root.face
          font.pixelSize: Commons.Style.font.bodySmall
          wrapMode: Text.WordWrap
        }

        Text {
          width: parent.width
          visible: root.view.failureKind !== ""
          text: root.view.failureCode && root.view.failureCode !== root.view.failureKind
            ? root.view.failureKind + "  " + root.view.failureCode + "  " + root.view.failureText
            : root.view.failureKind + "  " + root.view.failureText
          textFormat: Text.PlainText
          color: root.ink
          font.family: root.face
          font.pixelSize: Commons.Style.font.body
          wrapMode: Text.WordWrap
        }

        Text {
          width: parent.width
          text: root.view.snapshotLine
          textFormat: Text.PlainText
          color: root.ink
          opacity: root.view.dimmed ? 0.55 : 1
          font.family: root.face
          font.pixelSize: Commons.Style.font.bodySmall
          wrapMode: Text.WordWrap
        }

        Text {
          width: parent.width
          text: root.view.configLine
          textFormat: Text.PlainText
          color: root.ink
          opacity: root.view.dimmed ? 0.55 : 1
          font.family: root.face
          font.pixelSize: Commons.Style.font.bodySmall
          wrapMode: Text.WordWrap
        }

        Text {
          width: parent.width
          visible: root.view.vaultId !== ""
          text: "vault  " + root.view.vaultState + "  " + root.view.vaultId
          textFormat: Text.PlainText
          color: root.ink
          opacity: root.view.dimmed ? 0.55 : 1
          font.family: root.face
          font.pixelSize: Commons.Style.font.bodySmall
          wrapMode: Text.WordWrap
        }

        Repeater {
          model: root.view.volumes

          Column {
            required property var modelData
            width: content.width
            spacing: Commons.Style.space(2)

            Text {
              width: parent.width
              text: modelData.label + "  " + modelData.state
              textFormat: Text.PlainText
              color: root.ink
              opacity: root.view.dimmed ? 0.55 : 1
              font.family: root.face
              font.pixelSize: Commons.Style.font.body
              wrapMode: Text.WordWrap
            }

            Text {
              width: parent.width
              text: modelData.space
              textFormat: Text.PlainText
              color: root.ink
              opacity: root.view.dimmed ? 0.55 : 1
              font.family: root.face
              font.pixelSize: Commons.Style.font.bodySmall
              wrapMode: Text.WordWrap
            }

            Text {
              width: parent.width
              text: "path  " + modelData.path
              textFormat: Text.PlainText
              color: root.ink
              opacity: root.view.dimmed ? 0.55 : 1
              font.family: root.face
              font.pixelSize: Commons.Style.font.bodySmall
              wrapMode: Text.WordWrap
            }
          }
        }

        Text {
          width: parent.width
          text: root.view.preservedNote
          textFormat: Text.PlainText
          color: root.ink
          font.family: root.face
          font.pixelSize: Commons.Style.font.bodySmall
          wrapMode: Text.WordWrap
        }

        Repeater {
          model: root.view.preserved

          Column {
            required property var modelData
            width: content.width
            spacing: Commons.Style.space(2)

            Text {
              width: parent.width
              text: modelData.repo + "  " + modelData.revision + "  " + modelData.volume
              textFormat: Text.PlainText
              color: root.ink
              opacity: root.view.dimmed ? 0.55 : 1
              font.family: root.face
              font.pixelSize: Commons.Style.font.bodySmall
              wrapMode: Text.WordWrap
            }

            Text {
              width: parent.width
              text: modelData.provenance
              textFormat: Text.PlainText
              color: root.ink
              opacity: root.view.dimmed ? 0.55 : 1
              font.family: root.face
              font.pixelSize: Commons.Style.font.caption
              wrapMode: Text.WordWrap
            }
          }
        }

        Text {
          width: parent.width
          visible: root.view.transcript !== ""
          text: root.view.transcript
          textFormat: Text.PlainText
          color: root.ink
          opacity: 0.7
          font.family: root.face
          font.pixelSize: Commons.Style.font.caption
          wrapMode: Text.WordWrap
          maximumLineCount: 8
          elide: Text.ElideRight
        }

        Row {
          spacing: Commons.Style.space(16)

          Repeater {
            model: [
              { action: "refresh", label: "Refresh" },
              { action: "copy", label: "Copy arguments" },
              { action: "inspect", label: "Open inspection" }
            ]

            Text {
              required property var modelData
              text: modelData.label
              textFormat: Text.PlainText
              color: root.ink
              font.family: root.face
              font.pixelSize: Commons.Style.font.body
              font.underline: actionMouse.containsMouse

              MouseArea {
                id: actionMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.run(modelData.action)
              }
            }
          }
        }

        Text {
          width: parent.width
          visible: root.notice !== ""
          text: root.notice
          textFormat: Text.PlainText
          color: root.ink
          font.family: root.face
          font.pixelSize: Commons.Style.font.bodySmall
          wrapMode: Text.WordWrap
        }
      }
    }
  }
}
