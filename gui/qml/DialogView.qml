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
            case "commands": return commands
            case "input": return input
            case "confirm": return confirm
            case "choose": return choose
            case "message": return message
            case "viewer": return viewer
            case "jobs": return jobs
            case "settings": return settings
            default: return null
            }
        }
    }

    // A list with a highlighted row that stays in sight.
    component CursorList: ListView {
        id: list
        property int cursor: 0
        property var stateOf: (item) => ""
        // Rows laid out in columns (drives, bookmarks) need a fixed-width font.
        property bool fixed: false
        Layout.fillWidth: true
        Layout.preferredHeight: Math.min(contentHeight, dialogs.height * 0.6)
        // Columns in a fixed-width font may need more than the usual width
        // (the disk table); the dialog is capped by the window.
        implicitWidth: {
            var longest = 0
            var rows = model ? model : []
            for (var i = 0; i < rows.length; i++)
                longest = Math.max(longest, rows[i].length)
            return fixed ? Math.max(480, longest * rowMetrics.averageCharacterWidth + 32) : 480
        }
        FontMetrics {
            id: rowMetrics
            font: dialogs.theme.fixedFont
        }
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
                    font: list.fixed ? dialogs.theme.fixedFont : Qt.application.font
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
            hint: "F9 lists every command.    Esc: close"
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

    // The command list: category buttons, what was typed, then every
    // command as Category/Command with its shortcuts. Buttons and rows take
    // clicks too (MouseAreas, so the keyboard stays with the window).
    Component {
        id: commands
        Box {
            title: "Commands"
            hint: dialogs.view.hint
            hintColor: dialogs.view.recording ? dialogs.theme.accent : dialogs.theme.dim
            width: Math.min(dialogs.theme.width - 40, 860)
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: dialogs.view.categories
                    delegate: Rectangle {
                        readonly property bool chosen: index === dialogs.view.category
                        implicitWidth: name.implicitWidth + 20
                        implicitHeight: name.implicitHeight + 8
                        radius: 3
                        color: chosen ? palette.highlight : "transparent"
                        border.color: chosen ? palette.highlight : dialogs.theme.lineColor
                        Label {
                            id: name
                            anchors.centerIn: parent
                            text: modelData
                            color: chosen ? palette.highlightedText : palette.windowText
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: dialogs.theme.app.pick("category", index)
                        }
                    }
                }
            }
            Rectangle {
                Layout.fillWidth: true
                Layout.topMargin: 4
                implicitHeight: dialogs.theme.rowHeight + 6
                color: palette.base
                border.color: palette.highlight
                Label {
                    anchors.verticalCenter: parent.verticalCenter
                    x: 6
                    text: dialogs.view.recording
                          ? "Press the new shortcut for " + dialogs.view.recording
                          : "> " + dialogs.view.filter + "_"
                    color: dialogs.view.recording ? dialogs.theme.accent : palette.text
                    font: dialogs.theme.fixedFont
                }
            }
            ListView {
                id: rows
                Layout.fillWidth: true
                // A fixed height (SC-69): the window keeps its size whatever
                // the filter leaves in the list.
                Layout.preferredHeight: Math.min(16 * dialogs.theme.rowHeight, dialogs.height * 0.6)
                implicitWidth: 600
                interactive: false
                clip: true
                model: dialogs.view.rows
                currentIndex: dialogs.view.item
                onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)
                onCountChanged: positionViewAtIndex(currentIndex, ListView.Contain)
                delegate: Rectangle {
                    readonly property bool current: index === dialogs.view.item
                    width: ListView.view.width
                    height: dialogs.theme.rowHeight
                    radius: 2
                    color: current ? palette.highlight : "transparent"
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 6
                        anchors.rightMargin: 6
                        spacing: 16
                        Label {
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                            text: modelData.name
                            color: current ? palette.highlightedText : palette.windowText
                        }
                        Label {
                            text: modelData.keys
                            font: dialogs.theme.fixedFont
                            color: current ? palette.highlightedText : dialogs.theme.dim
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: dialogs.theme.app.pick("command", index)
                    }
                }
            }
            // F1: what the highlighted command does. Its room is always
            // reserved, so showing or hiding it does not move the window.
            Label {
                Layout.fillWidth: true
                Layout.topMargin: 4
                Layout.preferredHeight: 2 * dialogs.theme.rowHeight
                verticalAlignment: Text.AlignTop
                clip: true
                text: dialogs.view.rows.length === 0 ? "Nothing matches" : dialogs.view.help
                wrapMode: Text.Wrap
                color: dialogs.view.rows.length === 0 ? dialogs.theme.dim : dialogs.theme.accent
            }
        }
    }

    Component {
        id: input
        Box {
            title: dialogs.view.title
            hint: dialogs.view.hint
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
            // Multi-rename: the new names, following the template.
            Lines { model: dialogs.view.preview }
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
                fixed: true
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
            // About: the logo above the text.
            Image {
                visible: !!dialogs.view.logo_url
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: visible ? 128 : 0
                Layout.preferredHeight: visible ? 128 : 0
                source: dialogs.view.logo_url
                        ? (dialogs.theme.dark ? dialogs.view.logo_url : dialogs.view.logo_url_light)
                        : ""
                fillMode: Image.PreserveAspectFit
                smooth: true
                mipmap: true
            }
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
                  + "[/]: files at once    a: auto    x: remove finished    Esc: close"
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

    // Ctrl+,: options and shortcuts, changed in place. A dot marks values
    // that differ from the built-in ones.
    Component {
        id: settings
        Box {
            title: "Settings    " + dialogs.view.percent + "%"
            hint: dialogs.view.hint
            hintColor: dialogs.view.recording ? dialogs.theme.accent : dialogs.theme.dim
            width: Math.min(dialogs.theme.width - 40, 860)
            Repeater {
                model: dialogs.view.rows
                delegate: Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: dialogs.theme.rowHeight
                    readonly property bool current: index === dialogs.view.cursor
                    readonly property bool header: modelData.kind === "header"
                    color: current ? palette.highlight : "transparent"
                    radius: 2
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: header ? 0 : 12
                        anchors.rightMargin: 6
                        spacing: 12
                        Label {
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                            text: modelData.label
                            font.bold: header
                            color: current ? palette.highlightedText
                                 : header ? dialogs.theme.headingColor
                                 : modelData.kind === "command" ? dialogs.theme.linkColor
                                 : palette.windowText
                        }
                        Label {
                            visible: !header && modelData.kind !== "command"
                            Layout.preferredWidth: parent.width * 0.38
                            elide: Text.ElideRight
                            text: (modelData.changed ? "• " : "") + modelData.value
                            font: dialogs.theme.fixedFont
                            color: current ? palette.highlightedText
                                 : modelData.changed ? dialogs.theme.selectedText
                                 : modelData.value === "none" ? dialogs.theme.dim
                                 : palette.windowText
                        }
                    }
                }
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
