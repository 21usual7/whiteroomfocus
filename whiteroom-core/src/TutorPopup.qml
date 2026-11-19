import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Wayland

PanelWindow {
  id: popup

  required property string cli
  required property string session
  required property bool inSession
  signal dismissRequested()

  property var concepts: []
  property var messages: []
  property var preview: null
  property string awaiting: ""
  property string notice: ""
  property bool ready: false

  anchors { top: true; right: true }
  margins { top: inSession ? 54 : 12; right: 14 }
  implicitWidth: Math.min(420, (screen?.width ?? 448) - 28)
  implicitHeight: Math.min(470, (screen?.height ?? 540) - 75)
  color: "transparent"
  exclusionMode: ExclusionMode.Normal
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
  WlrLayershell.namespace: "whiteroom-tutor"

  function request(method, input) {
    if (!bridge.running || awaiting) return;
    awaiting = method;
    bridge.write(JSON.stringify(Object.assign({ method: method }, input)) + "\n");
  }

  function prepare(question, conceptId, claude) {
    notice = "";
    request("prepare", { question: question, conceptId: conceptId, claude: Boolean(claude) });
  }

  function confirm() {
    const draft = preview;
    preview = null;
    request("send", { previewId: draft.previewId });
  }

  function cancel() {
    const draft = preview;
    preview = null;
    request("cancel", { previewId: draft.previewId });
  }

  onVisibleChanged: {
    if (visible) {
      ready = false;
      preview = null;
      notice = "";
      awaiting = "";
      concepts = [];
      messages = [];
      composer.text = "";
    }
  }

  Process {
    id: bridge
    command: [popup.cli, "tutor-stdio", popup.session]
    stdinEnabled: true
    running: popup.visible
    onStarted: popup.request("context", {})
    onExited: {
      popup.awaiting = "";
      popup.ready = false;
      if (popup.visible) popup.notice = "Тьютор недоступен. Закрой и открой окно заново.";
    }
    stdout: SplitParser {
      onRead: data => {
        if (!popup.visible) return;
        const method = popup.awaiting;
        popup.awaiting = "";
        const response = JSON.parse(data);
        if (response.error) {
          popup.notice = response.error;
          if (popup.ready) Qt.callLater(() => composer.forceActiveFocus());
          return;
        }
        const value = response.value;
        if (method === "context") {
          const index = value.concepts.findIndex(concept => concept.id === value.suggestedConceptId);
          const selection = selector.currentValue;
          const existing = value.concepts.findIndex(concept => concept.id === selection);
          popup.concepts = value.concepts;
          selector.currentIndex = existing >= 0 ? existing : Math.max(index, 0);
          popup.messages = value.messages;
          popup.ready = true;
          Qt.callLater(() => {
            history.positionViewAtEnd();
            composer.forceActiveFocus();
          });
        } else if (method === "prepare") {
          if (value.local) popup.request("send", { previewId: value.previewId });
          else {
            popup.preview = value;
            Qt.callLater(() => confirmButton.forceActiveFocus());
          }
        } else if (method === "send") {
          composer.clear();
          popup.request("context", {});
        } else if (method === "cancel") {
          Qt.callLater(() => composer.forceActiveFocus());
        }
      }
    }
  }

  Shortcut {
    sequence: "Escape"
    enabled: popup.visible
    onActivated: popup.dismissRequested()
  }

  component Action: Button {
    id: button
    padding: 8
    contentItem: Text {
      text: button.text
      color: button.enabled ? "#e5f8f3" : "#829590"
      font.pixelSize: 13
      horizontalAlignment: Text.AlignHCenter
      verticalAlignment: Text.AlignVCenter
    }
    background: Rectangle {
      radius: 7
      color: button.down ? "#38564f" : button.hovered ? "#2e4a44" : "#243b36"
      border.color: button.activeFocus ? "#9dd9c9" : "#45635c"
    }
  }

  Rectangle {
    anchors.fill: parent
    radius: 12
    color: "#152022"
    border.color: "#53736c"

    ColumnLayout {
      anchors.fill: parent
      anchors.margins: 13
      spacing: 8

      RowLayout {
        Layout.fillWidth: true

        ColumnLayout {
          Layout.fillWidth: true
          spacing: 2

          Text {
            text: "Тьютор"
            color: "#defbf4"
            font.bold: true
            font.pixelSize: 18
          }

          Text {
            text: popup.inSession ? "Теория · локально" : "Теория и примеры"
            color: "#a6bbb5"
            font.pixelSize: 12
          }
        }

        Action {
          text: "Закрыть"
          onClicked: popup.dismissRequested()
        }
      }

      ListView {
        id: history
        Layout.fillWidth: true
        Layout.fillHeight: true
        spacing: 8
        clip: true
        model: popup.messages

        ScrollBar.vertical: ScrollBar {}

        delegate: Item {
          id: row
          required property var modelData
          readonly property bool fromUser: modelData.role === "user"
          readonly property bool fromClaude: modelData.role === "claude"
          width: ListView.view.width - 8
          height: line.implicitHeight + 20

          Rectangle {
            width: Math.round(row.width * 0.92)
            height: parent.height
            x: row.fromUser ? parent.width - width : 0
            radius: 9
            color: row.fromUser ? "#28483f" : "#203236"
            border.width: row.fromClaude ? 1 : 0
            border.color: "#a6dccc"

            Text {
              id: line
              anchors.fill: parent
              anchors.margins: 10
              textFormat: Text.PlainText
              text: row.modelData.text
              color: "#e3f4f0"
              font.pixelSize: 14
              wrapMode: Text.Wrap
            }
          }
        }

        Text {
          visible: !history.count && popup.ready
          anchors.centerIn: parent
          width: Math.max(0, history.width - 20)
          text: "Выбери понятие или задай вопрос по теории."
          color: "#a6bbb5"
          font.pixelSize: 14
          wrapMode: Text.Wrap
          horizontalAlignment: Text.AlignHCenter
        }
      }

      Text {
        Layout.fillWidth: true
        visible: text !== ""
        textFormat: Text.PlainText
        text: popup.notice || (popup.awaiting === "send" ? "Тьютор отвечает…" : "")
        color: "#e9cfa2"
        font.pixelSize: 13
        wrapMode: Text.Wrap
      }

      ColumnLayout {
        visible: popup.preview !== null
        Layout.fillWidth: true
        spacing: 7

        Text {
          Layout.fillWidth: true
          text: "Запрос к Claude · около " + (popup.preview?.estimatedTokens ?? 0) + " токенов"
          color: "#defbf4"
          font.pixelSize: 13
          wrapMode: Text.Wrap
        }

        ScrollView {
          Layout.fillWidth: true
          Layout.preferredHeight: 90
          clip: true
          TextArea {
            readOnly: true
            textFormat: TextEdit.PlainText
            text: popup.preview?.requestJson ?? ""
            color: "#defbf4"
            font.family: "monospace"
            font.pixelSize: 12
            wrapMode: TextEdit.Wrap
            background: Rectangle { color: "#1b3030"; radius: 7 }
          }
        }

        RowLayout {
          Action { id: confirmButton; text: "Отправить в Claude"; onClicked: popup.confirm() }
          Action { text: "Отмена"; onClicked: popup.cancel() }
        }
      }

      RowLayout {
        visible: popup.preview === null
        enabled: popup.ready && !popup.awaiting
        Layout.fillWidth: true

        ComboBox {
          id: selector
          Layout.fillWidth: true
          Layout.minimumWidth: 0
          model: popup.concepts
          textRole: "title"
          valueRole: "id"
          palette.text: "#defbf4"
          palette.buttonText: "#defbf4"
          palette.window: "#203432"
          palette.base: "#203432"
          background: Rectangle { color: "#243b36"; radius: 7; border.color: "#45635c" }
          Accessible.name: "Понятие"
        }

        Action {
          text: "Определение"
          enabled: Boolean(selector.currentValue)
          onClicked: popup.prepare(null, selector.currentValue)
        }

        Action {
          text: "Пример"
          enabled: Boolean(selector.currentValue)
          onClicked: popup.prepare("Пример: " + selector.currentText, selector.currentValue)
        }
      }

      RowLayout {
        visible: popup.preview === null
        enabled: popup.ready && !popup.awaiting
        Layout.fillWidth: true

        TextField {
          id: composer
          Layout.fillWidth: true
          Layout.minimumWidth: 0
          placeholderText: "Что такое корреляция?"
          placeholderTextColor: "#a6bbb5"
          color: "#defbf4"
          maximumLength: 300
          selectByMouse: true
          Accessible.name: "Вопрос по теории"
          background: Rectangle {
            radius: 7
            color: "#1b3030"
            border.color: composer.activeFocus ? "#a6dccc" : "#45635c"
          }
          onAccepted: if (text.trim()) popup.prepare(text.trim(), null)
        }

        Action {
          text: "Спросить"
          enabled: Boolean(composer.text.trim())
          onClicked: popup.prepare(composer.text.trim(), null)
        }

        Action {
          text: "Claude"
          enabled: Boolean(composer.text.trim())
          Accessible.name: "Спросить Claude свободным текстом"
          onClicked: popup.prepare(composer.text.trim(), null, true)
        }
      }
    }
  }
}
