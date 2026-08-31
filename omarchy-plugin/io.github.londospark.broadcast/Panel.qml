import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Ui
import qs.Commons
import "Model.js" as Model

Panel {
  id: root
  moduleName: "io.github.londospark.broadcast"
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  property var status: null
  property var apps: []
  property var devices: ({ output: [], input: [] })
  property var hyprClients: []

  property var actionQueue: []
  property bool actionBusy: false
  property string lastMessage: ""
  property bool lastMessageIsError: false

  readonly property bool active: !!(status && status.active)
  readonly property string backendValue: status ? status.backend : "deepfilter"
  readonly property bool maxineAvailable: !!(status && status.maxine_available)
  readonly property real intensity: status && status.maxine_intensity !== undefined ? status.maxine_intensity : 1.0
  readonly property string healthText: Model.healthLabel(status)
  readonly property color healthColor: !active
    ? Qt.darker(root.barForeground, 1.6)
    : (status && status.health === "ok" ? "#6fbf73" : (root.bar ? root.bar.urgent : "#c0645a"))

  function open() { root.controller.show() }
  function close() { root.controller.hide() }

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.hostWidget || root, direction)
    return false
  }

  function refreshStatus() { if (!statusProc.running) statusProc.running = true }
  function refreshApps() { if (root.opened && !appsProc.running) appsProc.running = true }
  function refreshDevices() { if (root.opened && !devicesProc.running) devicesProc.running = true }
  function refreshHyprClients() { if (root.opened && !hyprClientsProc.running) hyprClientsProc.running = true }
  function refreshAll() { refreshStatus(); refreshApps(); refreshDevices(); refreshHyprClients() }

  // Chromium-based browsers give every window/tab's audio stream the exact
  // same name ("Brave", "Playback") with no way to tell them apart from
  // PipeWire metadata alone. Hyprland's window titles are the best
  // available stand-in — when the count of same-named streams matches the
  // count of that app's open windows, pair them up positionally and show
  // the real title; otherwise fall back to a plain ordinal rather than
  // risk showing a title next to the wrong stream.
  function duplicateSuffix(app) {
    var name = app.name || app.binary
    var group = []
    for (var i = 0; i < root.apps.length; i++) {
      var a = root.apps[i]
      if ((a.name || a.binary) === name) group.push(a)
    }
    if (group.length <= 1) return ""

    var ordinal = 0
    for (var j = 0; j < group.length; j++) {
      if (group[j].id === app.id) ordinal = j + 1
    }

    var titles = Model.browserWindowTitles(app.binary, root.hyprClients)
    if (titles.length === group.length) {
      var title = titles[ordinal - 1]
      return " · " + (title.length > 46 ? title.slice(0, 45) + "…" : title)
    }
    return " · window " + ordinal
  }

  function deviceOptions(list) {
    var opts = [{ value: "auto", label: "Auto-detect" }]
    var arr = list || []
    for (var i = 0; i < arr.length; i++)
      opts.push({ value: arr[i].name, label: arr[i].description || arr[i].name })
    return opts
  }

  // Queue commands so a backend switch (set-backend, then install-config
  // --apply) runs sequentially rather than racing two broadcast-ctl
  // invocations against the same state file.
  function runActions(commands, note) {
    actionQueue = commands.slice()
    lastMessage = note || ""
    lastMessageIsError = false
    messageTimer.stop()
    runNext()
  }

  function runNext() {
    if (actionQueue.length === 0) {
      actionBusy = false
      refreshAll()
      if (lastMessage) messageTimer.restart()
      return
    }
    actionBusy = true
    var cmd = actionQueue.shift()
    actionProc.command = cmd
    actionProc.running = true
  }

  onOpenedChanged: {
    if (opened) {
      refreshAll()
      refreshTimer.restart()
    } else {
      refreshTimer.stop()
    }
  }

  Process {
    id: statusProc
    command: ["broadcast-ctl", "status", "--json"]
    stdout: StdioCollector {
      id: statusOut
      waitForEnd: true
      onStreamFinished: {
        var parsed = Model.parseStatus(statusOut.text)
        if (parsed) root.status = parsed
      }
    }
  }

  Process {
    id: appsProc
    command: ["broadcast-ctl", "apps", "--json"]
    stdout: StdioCollector {
      id: appsOut
      waitForEnd: true
      onStreamFinished: root.apps = Model.parseApps(appsOut.text)
    }
  }

  Process {
    id: devicesProc
    command: ["broadcast-ctl", "devices", "--json"]
    stdout: StdioCollector {
      id: devicesOut
      waitForEnd: true
      onStreamFinished: {
        var parsed = Model.parseStatus(devicesOut.text)
        if (parsed) root.devices = parsed
      }
    }
  }

  Process {
    id: hyprClientsProc
    command: ["hyprctl", "clients", "-j"]
    stdout: StdioCollector {
      id: hyprClientsOut
      waitForEnd: true
      onStreamFinished: root.hyprClients = Model.parseHyprClients(hyprClientsOut.text)
    }
  }

  Process {
    id: actionProc
    stdout: StdioCollector { id: actionOut; waitForEnd: true }
    stderr: StdioCollector { id: actionErr; waitForEnd: true }
    onExited: function(exitCode, exitStatus) {
      if (exitCode !== 0) {
        root.lastMessageIsError = true
        root.lastMessage = (actionErr.text || "Command failed").trim() || "Command failed"
        root.actionQueue = []
      }
      root.runNext()
    }
  }

  Timer {
    id: refreshTimer
    interval: 4000
    repeat: true
    onTriggered: root.refreshAll()
  }

  Timer {
    id: messageTimer
    interval: 4000
    onTriggered: root.lastMessage = ""
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root.hostWidget || root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(360))
    contentHeight: panel.fittedContentHeight(panelColumn.implicitHeight, Style.space(620))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      ScrollView {
        id: scrollArea
        anchors.fill: parent
        clip: true
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
        ScrollBar.vertical.policy: panelColumn.implicitHeight > height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff

        Column {
          id: panelColumn
          width: scrollArea.availableWidth
          spacing: Style.space(14)

          // ---------- Hero ----------
          PanelHero {
            width: parent.width
            title: "Broadcast"
            meta: root.status
              ? (Model.backendLabel(root.backendValue) + " · " + root.healthText)
              : "Loading…"
            detail: root.backendValue === "maxine" ? "GPU" : "CPU"
            foreground: root.barForeground
            fontFamily: root.bar ? root.bar.fontFamily : Style.font.family

            iconComponent: Component {
              Item {
                implicitWidth: Style.font.display
                implicitHeight: Style.font.display
                Text {
                  anchors.centerIn: parent
                  text: root.active ? "󰍬" : "󰍭"
                  color: root.healthColor
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.display
                }
              }
            }

            trailingControl: Component {
              ToggleSwitch {
                checked: root.active
                busy: root.actionBusy
                foreground: root.barForeground
                onToggled: root.runActions([["broadcast-ctl", "toggle"]], root.active ? "Turning off…" : "Turning on…")
              }
            }
          }

          Text {
            visible: root.lastMessage !== ""
            width: parent.width
            text: root.lastMessage
            color: root.lastMessageIsError ? (root.bar ? root.bar.urgent : "#c0645a") : Qt.darker(root.barForeground, 1.3)
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.bodySmall
            wrapMode: Text.WordWrap
          }

          // ---------- Backend ----------
          PanelSeparator { foreground: root.barForeground }

          Column {
            width: parent.width
            spacing: Style.space(8)

            PanelSectionHeader {
              text: "BACKEND"
              foreground: root.barForeground
              fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
            }

            ButtonGroup {
              width: parent.width
              foreground: root.barForeground
              accent: Color.accent
              fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
              value: root.backendValue
              options: [
                { value: "deepfilter", label: "DeepFilterNet", tooltip: "CPU noise suppression — works on any hardware" },
                { value: "maxine", label: "Maxine (GPU)", tooltip: root.maxineAvailable ? "NVIDIA RTX Tensor Cores" : "Requires the Maxine SDK — not installed yet" }
              ]
              onChanged: function(v) {
                if (v === root.backendValue) return
                root.runActions(
                  [["broadcast-ctl", "set-backend", v], ["broadcast-ctl", "install-config", "--apply"]],
                  "Switching to " + Model.backendLabel(v) + "… (PipeWire will restart)"
                )
              }
            }

            Text {
              visible: root.backendValue === "maxine" && !root.maxineAvailable
              width: parent.width
              text: "Maxine LADSPA plugin not found. Run scripts/install-maxine-sdk.sh with an NGC API key, then switch backends again."
              color: root.bar ? root.bar.urgent : "#c0645a"
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }

            Column {
              visible: root.backendValue === "maxine"
              width: parent.width
              spacing: Style.space(4)

              Item {
                width: parent.width
                implicitHeight: intensityLabel.implicitHeight

                Text {
                  id: intensityLabel
                  text: "INTENSITY"
                  color: Qt.darker(root.barForeground, 1.4)
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.caption
                  font.bold: true
                  anchors.left: parent.left
                }

                Text {
                  text: Math.round((intensitySlider.dragging ? intensitySlider.liveValue : root.intensity) * 100) + "%"
                  color: Qt.darker(root.barForeground, 1.4)
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.caption
                  font.bold: true
                  anchors.right: parent.right
                }
              }

              PanelSlider {
                id: intensitySlider
                bar: root.bar
                width: parent.width
                minimum: 0
                maximum: 1
                step: 0.05
                value: root.intensity
                onReleased: function(v) {
                  root.runActions([["broadcast-ctl", "set-intensity", String(Math.round(v * 100))]], "Intensity → " + Math.round(v * 100) + "%")
                }
              }
            }
          }

          // ---------- Health ----------
          PanelSeparator { foreground: root.barForeground }

          Column {
            width: parent.width
            spacing: Style.space(8)

            Item {
              width: parent.width
              implicitHeight: Math.max(healthHeader.implicitHeight, fixButton.implicitHeight)

              PanelSectionHeader {
                id: healthHeader
                text: "HEALTH"
                foreground: root.barForeground
                fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
              }

              Button {
                id: fixButton
                text: "Fix routing"
                bordered: true
                foreground: root.barForeground
                accent: Color.accent
                fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
                fontSize: Style.font.caption
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                onClicked: root.runActions([["broadcast-ctl", "fix-routing"]], "Repairing routing…")
              }
            }

            Row {
              spacing: Style.space(8)

              Rectangle {
                width: Style.space(9)
                height: Style.space(9)
                radius: width / 2
                anchors.verticalCenter: parent.verticalCenter
                color: root.healthColor
              }

              Text {
                text: root.healthText
                color: root.barForeground
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.body
                font.bold: true
              }
            }

            Column {
              width: parent.width
              spacing: Style.space(3)
              visible: root.status && root.status.issues && root.status.issues.length > 0

              Repeater {
                model: root.status && root.status.issues ? root.status.issues : []

                Text {
                  required property string modelData
                  width: parent.width
                  text: "⚠ " + modelData
                  color: Qt.darker(root.barForeground, 1.2)
                  font.family: root.bar ? root.bar.fontFamily : Style.font.family
                  font.pixelSize: Style.font.caption
                  wrapMode: Text.WordWrap
                }
              }
            }
          }

          // ---------- Devices ----------
          PanelSeparator { foreground: root.barForeground }

          Column {
            width: parent.width
            spacing: Style.space(8)

            PanelSectionHeader {
              text: "DEVICES"
              foreground: root.barForeground
              fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
            }

            Column {
              width: parent.width
              spacing: Style.space(2)

              Text {
                text: "Output (speakers)"
                color: Qt.darker(root.barForeground, 1.3)
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.caption
              }

              Dropdown {
                width: parent.width
                height: implicitHeight
                showLabel: false
                foreground: root.barForeground
                background: Color.popups.background
                accent: Color.accent
                fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
                options: root.deviceOptions(root.devices.output)
                value: (root.status && root.status.preferred_output_sink) || "auto"
                onChanged: function(v) {
                  root.runActions([["broadcast-ctl", "set-device", "output", v]], "Output device updated")
                }
              }
            }

            Column {
              width: parent.width
              spacing: Style.space(2)

              Text {
                text: "Input (microphone)"
                color: Qt.darker(root.barForeground, 1.3)
                font.family: root.bar ? root.bar.fontFamily : Style.font.family
                font.pixelSize: Style.font.caption
              }

              Dropdown {
                width: parent.width
                height: implicitHeight
                showLabel: false
                foreground: root.barForeground
                background: Color.popups.background
                accent: Color.accent
                fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
                options: root.deviceOptions(root.devices.input)
                value: (root.status && root.status.preferred_input_source) || "auto"
                onChanged: function(v) {
                  root.runActions([["broadcast-ctl", "set-device", "input", v]], "Input device updated")
                }
              }
            }
          }

          // ---------- Apps ----------
          PanelSeparator { foreground: root.barForeground }

          Column {
            width: parent.width
            spacing: Style.space(6)

            PanelSectionHeader {
              text: "APPS"
              foreground: root.barForeground
              fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
            }

            Text {
              visible: root.apps.length === 0
              width: parent.width
              text: "No audio streams playing."
              color: Qt.darker(root.barForeground, 1.4)
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.bodySmall
            }

            Repeater {
              model: root.apps

              CursorSurface {
                id: appRow
                required property var modelData
                width: panelColumn.width
                foreground: root.barForeground
                fill: Style.hoverFillFor(root.barForeground, Color.accent)
                implicitHeight: appInner.implicitHeight + Style.spacing.lg

                Row {
                  id: appInner
                  anchors.left: parent.left
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                  anchors.leftMargin: Style.space(8)
                  anchors.rightMargin: Style.space(8)
                  spacing: Style.space(8)

                  Text {
                    text: (appRow.modelData.name || appRow.modelData.binary) + root.duplicateSuffix(appRow.modelData)
                    color: root.barForeground
                    font.family: root.bar ? root.bar.fontFamily : Style.font.family
                    font.pixelSize: Style.font.body
                    elide: Text.ElideRight
                    width: appInner.width - appSwitch.width - appInner.spacing
                    anchors.verticalCenter: parent.verticalCenter
                  }

                  ToggleSwitch {
                    id: appSwitch
                    checked: appRow.modelData.route === "filtered"
                    foreground: root.barForeground
                    onToggled: {
                      // Routed by this stream's own PipeWire id, not by app
                      // name — apps like browsers run every window/tab
                      // through one shared audio process with identical
                      // names, so routing by name would move all of them
                      // together. This only affects this one stream, for
                      // this session (see broadcast-ctl route-id --help).
                      var next = appRow.modelData.route === "filtered" ? "direct" : "filtered"
                      root.runActions(
                        [["broadcast-ctl", "route-id", String(appRow.modelData.id), next]],
                        (appRow.modelData.name || appRow.modelData.binary) + (next === "filtered" ? " → filtered" : " → direct")
                      )
                    }
                  }
                }

                MouseArea {
                  anchors.fill: parent
                  acceptedButtons: Qt.NoButton
                  hoverEnabled: true
                  onContainsMouseChanged: appRow.hasCursor = containsMouse
                }
              }
            }
          }
        }
      }
    }
  }
}
