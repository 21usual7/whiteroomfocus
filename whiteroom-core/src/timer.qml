import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland

ShellRoot {
  id: root

  readonly property string session: Quickshell.env("WHITEROOM_TIMER_ID")
  readonly property string cli: Quickshell.env("WHITEROOM_TIMER_CLI")
  readonly property bool inSession: session !== "idle"
  readonly property real started: Number(Quickshell.env("WHITEROOM_TIMER_STARTED"))
  property string elapsed: "00:00:00"
  property string popupMonitor: ""
  property bool confirmStop: false

  function showPopup(monitor) {
    popupMonitor = monitor || Hyprland.focusedMonitor?.name || Quickshell.screens[0]?.name || "";
  }

  function closePopup() {
    popupMonitor = "";
    if (!inSession) Qt.quit();
  }

  IpcHandler {
    target: "tutor"
    function ping(): void {}
    function open(): void { root.showPopup(""); }
  }

  Timer {
    interval: 250
    running: root.inSession
    repeat: true
    triggeredOnStart: true
    onTriggered: {
      const seconds = Math.max(0, Math.floor(Date.now() / 1000) - root.started);
      root.elapsed = [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60]
        .map(part => String(part).padStart(2, "0")).join(":");
    }
  }

  // Второе нажатие «Стоп» в течение 3 секунд заканчивает сессию.
  Timer {
    id: confirmReset
    interval: 3000
    onTriggered: root.confirmStop = false
  }

  Process {
    id: stopper
    command: [root.cli, "stop"]
  }

  Process {
    command: [root.cli, "timer-watch", root.session]
    running: true
    onExited: Qt.quit()
  }

  Variants {
    model: Quickshell.screens

    PanelWindow {
      required property var modelData
      screen: modelData
      visible: root.inSession
      anchors { top: true; right: true }
      margins { top: 12; right: 14 }
      implicitWidth: clock.implicitWidth + stopButton.implicitWidth + trigger.implicitWidth + 41
      implicitHeight: 34
      color: "transparent"
      exclusionMode: ExclusionMode.Normal
      WlrLayershell.layer: WlrLayer.Overlay
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
      WlrLayershell.namespace: "whiteroom-timer"
      mask: Region { item: stopButton; regions: [Region { item: trigger }] }

      Rectangle {
        anchors.fill: parent
        radius: 10
        color: "#eb12181a"
        border.color: "#486761"

        RowLayout {
          anchors.fill: parent
          anchors.leftMargin: 12
          anchors.rightMargin: 4
          spacing: 9

          Text {
            id: clock
            text: root.elapsed
            color: "#defbf4"
            font.family: "monospace"
            font.pixelSize: 18
          }

          Rectangle {
            id: stopButton
            Layout.preferredWidth: implicitWidth
            Layout.preferredHeight: 26
            implicitWidth: stopLabel.implicitWidth + 16
            radius: 6
            color: root.confirmStop ? "#7a3434" : stopMouse.containsMouse ? "#4a3131" : "#2e2424"

            Text {
              id: stopLabel
              anchors.centerIn: parent
              text: root.confirmStop ? "Точно?" : "Стоп"
              color: "#defbf4"
              font.pixelSize: 13
            }

            MouseArea {
              id: stopMouse
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: {
                if (root.confirmStop) {
                  root.confirmStop = false;
                  stopper.running = true;
                } else {
                  root.confirmStop = true;
                  confirmReset.restart();
                }
              }
            }
          }

          Rectangle {
            id: trigger
            Layout.preferredWidth: implicitWidth
            Layout.preferredHeight: 26
            implicitWidth: tutorLabel.implicitWidth + 16
            radius: 6
            color: tutorMouse.containsMouse || root.popupMonitor === modelData.name ? "#31514c" : "#233a37"

            Text {
              id: tutorLabel
              anchors.centerIn: parent
              text: "Тьютор"
              color: "#defbf4"
              font.pixelSize: 13
            }

            MouseArea {
              id: tutorMouse
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: {
                if (root.popupMonitor === modelData.name) root.closePopup();
                else root.showPopup(modelData.name);
              }
            }
          }
        }
      }
    }
  }

  Variants {
    model: Quickshell.screens

    TutorPopup {
      required property var modelData
      screen: modelData
      visible: root.popupMonitor === modelData.name
      inSession: root.inSession
      session: root.session
      cli: root.cli
      onDismissRequested: root.closePopup()
    }
  }
}
