// The open dialog or window over the panels (`view`, an OverlayView of the
// screen description, by `kind`), and above everything a copy conflict
// question (`conflict`). The hints name the keys, since the keyboard is
// how dialogs are answered.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: dialogs
    property var view: null
    property var conflict: null
    readonly property var theme: ApplicationWindow.window
    visible: view !== null || conflict !== null

    // Dims the panels behind a dialog.
    Rectangle {
        anchors.fill: parent
        color: "#000000"
        opacity: 0.25
    }

    // Sized by the dialog it shows.
    Loader {
        anchors.centerIn: parent
        active: dialogs.view !== null
        sourceComponent: {
            switch (dialogs.view ? dialogs.view.kind : "") {
            case "help": return help
            case "menu": return menu
            case "input": return input
            case "confirm": return confirm
            case "choose": return choose
            case "message": return message
            case "viewer": return viewer
            case "jobs": return jobs
            default: return null
            }
        }
    }

    // A list with a highlighted row that stays in sight.
    component CursorList: ListView {
        id: list
        property int cursor: 0
        property var stateOf: (item) => ""
        Layout.fillWidth: true
        Layout.preferredHeight: Math.min(contentHeight, dialogs.height * 0.6)
        implicitWidth: 480
        interactive: false
        clip: true
        onCursorChanged: positionViewAtIndex(cursor, ListView.Contain)
        onCountChanged: positionViewAtIndex(cursor, ListView.Contain)
        delegate: Component {
            Rectangle {
                width: ListView.view.width
                height: dialogs.theme.rowHeight
                color: index === list.cursor ? palette.highlight : "transparent"
                Label {
                    anchors.verticalCenter: parent.verticalCenter
                    x: 4
                    width: parent.width - 8
                    elide: Text.ElideRight
                    text: typeof modelData === "string" ? modelData : modelData.text
                    color: index === list.cursor ? palette.highlightedText
                         : list.stateOf(modelData) === "failed" ? dialogs.theme.errorColor
                         : palette.windowText
                }
            }
        }
    }

    component Lines: Repeater {
        delegate: Label {
            Layout.fillWidth: true
            text: modelData
            wrapMode: Text.Wrap
            textFormat: Text.PlainText
        }
    }

    Component {
        id: help
        Box {
            title: "Main shortcuts"
            hint: "Every command is also in the menu bar.    Esc: close"
            GridLayout {
                columns: 3
                columnSpacing: 32
                Repeater {
                    model: dialogs.view.groups
                    delegate: ColumnLayout {
                        Layout.alignment: Qt.AlignTop
                        Label {
                            text: modelData.title
                            font.bold: true
                            color: dialogs.theme.accent
                        }
                        Repeater {
                            model: modelData.rows
                            delegate: RowLayout {
                                spacing: 12
                                Label {
                                    text: modelData[0]
                                    font.bold: true
                                    Layout.preferredWidth: 110
                                }
                                Label { text: modelData[1] }
                            }
                        }
                    }
                }
            }
        }
    }

    Component {
        id: menu
        Box {
            title: "Menu"
            hint: "Left Right: menu    Up Down: command    Enter: run    Esc: close"
            RowLayout {
                spacing: 16
                Repeater {
                    model: dialogs.view.menus
                    delegate: Label {
                        text: modelData.title
                        font.bold: index === dialogs.view.menu
                        color: index === dialogs.view.menu ? dialogs.theme.accent
                                                           : palette.windowText
                    }
                }
            }
            CursorList {
                cursor: dialogs.view.item
                model: dialogs.view.menus[dialogs.view.menu].items.map(
                    (item) => item.shortcut ? item.label + "    " + item.shortcut : item.label)
            }
        }
    }

    Component {
        id: input
        Box {
            title: dialogs.view.title
            hint: "Enter: OK    Esc: cancel    Ctrl+U: clear"
            Label {
                text: dialogs.view.prompt
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: dialogs.theme.rowHeight + 6
                color: palette.base
                border.color: palette.highlight
                Text {
                    id: field
                    anchors.verticalCenter: parent.verticalCenter
                    x: 4
                    text: dialogs.view.text
                    font: dialogs.theme.fixedFont
                    color: palette.text
                    textFormat: Text.PlainText
                }
                TextMetrics {
                    id: beforeCursor
                    font: dialogs.theme.fixedFont
                    text: Array.from(dialogs.view.text).slice(0, dialogs.view.cursor).join("")
                }
                Rectangle {
                    x: 4 + beforeCursor.advanceWidth
                    anchors.verticalCenter: parent.verticalCenter
                    width: 2
                    height: dialogs.theme.rowHeight
                    color: palette.text
                }
            }
        }
    }

    Component {
        id: confirm
        Box {
            title: dialogs.view.title
            hint: "Enter / y: yes    Esc / n: no"
            Lines { model: dialogs.view.lines }
        }
    }

    Component {
        id: choose
        Box {
            title: dialogs.view.title
            hint: dialogs.view.footer
            CursorList {
                cursor: dialogs.view.cursor
                model: dialogs.view.items
            }
        }
    }

    Component {
        id: message
        Box {
            title: dialogs.view.title
            hint: dialogs.view.hint
            Lines { model: dialogs.view.lines }
        }
    }

    Component {
        id: viewer
        Rectangle {
            width: dialogs.width
            height: dialogs.height
            color: palette.base
            ViewerView {
                anchors.fill: parent
                anchors.margins: 8
                view: dialogs.view
            }
        }
    }

    Component {
        id: jobs
        Box {
            title: "Transfers"
            hint: "p: pause    r: resume/retry    c: cancel    +/-: priority    "
                  + "x: remove finished    Esc: close"
            Label {
                visible: dialogs.view.rows.length === 0
                text: "No transfers."
                color: dialogs.theme.dim
            }
            CursorList {
                cursor: dialogs.view.cursor
                model: dialogs.view.rows
                stateOf: (row) => row.state
            }
        }
    }

    // A copy conflict waits for an answer before anything else.
    Box {
        anchors.centerIn: parent
        visible: dialogs.conflict !== null
        title: "File exists"
        hint: "o: overwrite    s: skip    r: keep both    a / Esc: abort\n"
              + "Capital letter (O, S, R): same answer for all"
        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: dialogs.conflict ? dialogs.conflict.destination + " already exists." : ""
        }
        Label { text: dialogs.conflict ? "Existing:  " + dialogs.conflict.existing : "" }
        Label { text: dialogs.conflict ? "New:          " + dialogs.conflict.incoming : "" }
    }
}
