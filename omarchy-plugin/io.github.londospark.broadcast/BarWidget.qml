import QtQuick
import Quickshell
import Quickshell.Io
import qs.Ui
import "Model.js" as Model

// Bar icon for Broadcast: a mic glyph that reflects filter state at a
// glance (on/off, degraded), plus a popout Panel with full controls.
// Follows the same BarWidget+Loader(Panel) contract as the built-in
// microphone/audio widgets — see qs.Ui.BarWidget / KeyboardPanel.
BarWidget {
  id: root
  moduleName: "io.github.londospark.broadcast"

  property var status: null

  readonly property bool active: !!(status && status.active)
  readonly property bool degraded: active && status && status.health !== "ok"

  readonly property bool opened: panelLoader.item
    ? panelLoader.item.opened === true
    : false

  function open() { if (panelLoader.item) panelLoader.item.open() }
  function close() { if (panelLoader.item) panelLoader.item.close() }
  function toggle() { if (panelLoader.item) panelLoader.item.toggle() }
  function closeForPopoutSwitch() { if (panelLoader.item) panelLoader.item.closeForPopoutSwitch() }

  function injectPanel() {
    if (!panelLoader.item) return
    panelLoader.item.bar = root.bar
    panelLoader.item.anchorItem = button
    panelLoader.item.hostWidget = root
    panelLoader.item.status = Qt.binding(function() { return root.status })
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onBarChanged: injectPanel()

  function refreshStatus() {
    if (!statusProc.running) statusProc.running = true
  }

  Process {
    id: statusProc
    command: ["broadcast-ctl", "status", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var parsed = Model.parseStatus(text)
        if (parsed) root.status = parsed
      }
    }
  }

  Timer {
    interval: 5000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refreshStatus()
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

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.active ? "󰍬" : "󰍭"
    active: root.degraded
    dimmed: !root.active
    tooltipText: root.status
      ? ("Broadcast: " + (root.active ? "ON" : "OFF")
          + " · " + Model.backendLabel(root.status.backend)
          + " · " + Model.healthLabel(root.status))
      : "Broadcast"
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.MiddleButton) toggleProc.running = true
      else root.toggle()
    }
  }

  // Middle-click quick toggle, mirroring the built-in microphone widget's
  // fast path — no need to open the panel just to flip the master switch.
  Process {
    id: toggleProc
    command: ["broadcast-ctl", "toggle"]
    onExited: root.refreshStatus()
  }

  onOpenedChanged: if (opened) refreshStatus()
}
