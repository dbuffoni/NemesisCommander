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
        // The rows are as wide as the window; a longer row is cut in the
        // middle, where a path keeps its start and its end.
        property bool keepsSize: false
        Layout.fillWidth: true
        // Whole rows only: a row is never cut in half.
        Layout.preferredHeight: Math.max(1, Math.floor((keepsSize ? dialogs.height * 0.6
            : Math.min(contentHeight, dialogs.height * 0.6)) / dialogs.theme.rowHeight))
            * dialogs.theme.rowHeight
        implicitWidth: 480
        interactive: false
        clip: true
        onCursorChanged: positionViewAtIndex(cursor, ListView.Contain)
        onCountChanged: positionViewAtIndex(cursor, ListView.Contain)
        // Tables in the fixed font (disks, drives) shrink, down to 60%, so
        // that their columns are not cut.
        FontMetrics { id: tableMetrics; font: dialogs.theme.fixedFont }
        readonly property real shrink: {
            if (!fixed || !model || width <= 0)
                return 1
            var widest = 0 * tableMetrics.height
            for (var i = 0; i < model.length; i++)
                widest = Math.max(widest, tableMetrics.advanceWidth(
                    typeof model[i] === "string" ? model[i] : model[i].text))
            return widest <= width - 8 ? 1 : Math.max(0.6, (width - 8) / widest)
        }
        delegate: Component {
            Rectangle {
                width: ListView.view.width
                height: dialogs.theme.rowHeight
                color: index === list.cursor ? palette.highlight : "transparent"
                Label {
                    anchors.verticalCenter: parent.verticalCenter
                    x: 4
                    width: parent.width - 8
                    elide: Text.ElideMiddle
                    font: list.fixed ? Qt.font({ family: dialogs.theme.fixedFont.family,
                                                 pointSize: dialogs.theme.fixedFont.pointSize * list.shrink })
                                     : Qt.application.font
                    text: typeof modelData === "string" ? modelData : modelData.text
                    color: index === list.cursor ? palette.highlightedText
                         : list.stateOf(modelData) === "failed" ? dialogs.theme.errorColor
                         : palette.windowText
                }
            }
        }
    }

    component Lines: Repeater {
        // A table of examples keeps its columns: a line with a run of spaces
        // is cut with an ellipsis instead of wrapped.
        property bool table: false
        delegate: Label {
            readonly property bool columns: table && modelData.indexOf("  ", 2) >= 0
            Layout.fillWidth: true
            text: modelData
            wrapMode: columns ? Text.NoWrap : Text.Wrap
            elide: columns ? Text.ElideRight : Text.ElideNone
            textFormat: Text.PlainText
        }
    }

    // F1: the groups of shortcuts in as many columns (1 to 3) as the window
    // has room for. A key column and a description column per group, the
    // descriptions cut with an ellipsis when even one column is too narrow,
    // and the whole list scrolls when it is too tall.
    Component {
        id: help
        Box {
            id: helpBox
            title: "Main shortcuts"
            hint: dialogs.view.hint
            readonly property real gap: 32
            readonly property var bold: Qt.font({ family: dialogs.theme.font.family,
                                                  pointSize: dialogs.theme.font.pointSize,
                                                  bold: true })
            FontMetrics { id: boldMetrics; font: helpBox.bold }
            FontMetrics { id: plainMetrics; font: dialogs.theme.font }
            // The widest key text and the widest description of all groups.
            readonly property var widest: {
                // The metrics change with the font: depend on them.
                var key = 0, desc = 0, groups = dialogs.view.groups
                key += 0 * (boldMetrics.height + plainMetrics.height)
                for (var i = 0; i < groups.length; i++) {
                    key = Math.max(key, boldMetrics.advanceWidth(groups[i].title))
                    for (var j = 0; j < groups[i].rows.length; j++) {
                        key = Math.max(key, boldMetrics.advanceWidth(groups[i].rows[j][0]))
                        desc = Math.max(desc, plainMetrics.advanceWidth(groups[i].rows[j][1]))
                    }
                }
                return { key: Math.ceil(key) + 2, desc: Math.ceil(desc) + 2 }
            }
            readonly property real groupWidth: widest.key + 12 + widest.desc
            readonly property int wanted: Math.min(3, Math.max(1, dialogs.view.groups.length))
            width: Math.min(maxWidth, wanted * groupWidth + (wanted - 1) * gap + 32)
            readonly property real room: width - 32
            readonly property int cols: Math.max(1, Math.min(wanted,
                Math.floor((room + gap) / (groupWidth + gap))))
            readonly property real cellWidth: (room - (cols - 1) * gap) / cols
            readonly property real keyWidth: Math.min(widest.key, cellWidth * 0.55)
            Flickable {
                id: scroller
                Layout.fillWidth: true
                // Whole lines only, so the scroll never cuts a row in half.
                readonly property real lineH: Math.max(1, Math.ceil(boldMetrics.lineSpacing))
                Layout.preferredHeight: Math.min(grid.implicitHeight,
                    Math.max(3, Math.floor((dialogs.height * 0.9 - 120) / lineH)) * lineH)
                contentWidth: width
                contentHeight: grid.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar {}
                GridLayout {
                    id: grid
                    width: scroller.width
                    columns: helpBox.cols
                    columnSpacing: helpBox.gap
                    rowSpacing: 0
                    Repeater {
                        model: dialogs.view.groups
                        delegate: ColumnLayout {
                            Layout.alignment: Qt.AlignTop
                            Layout.preferredWidth: helpBox.cellWidth
                            Layout.maximumWidth: helpBox.cellWidth
                            spacing: 0
                            Label {
                                Layout.fillWidth: true
                                // A blank line between the groups of a column.
                                Layout.topMargin: index >= helpBox.cols ? scroller.lineH : 0
                                text: modelData.title
                                font.bold: true
                                elide: Text.ElideRight
                                color: dialogs.theme.accent
                            }
                            Repeater {
                                model: modelData.rows
                                delegate: RowLayout {
                                    spacing: 12
                                    Label {
                                        text: modelData[0]
                                        font.bold: true
                                        elide: Text.ElideRight
                                        Layout.preferredWidth: helpBox.keyWidth
                                        Layout.maximumWidth: helpBox.keyWidth
                                    }
                                    Label {
                                        Layout.fillWidth: true
                                        text: modelData[1]
                                        elide: Text.ElideRight
                                    }
                                }
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
            width: Math.min(dialogs.theme.width * 0.9, 1000)
            fixedHeight: Math.min(dialogs.theme.height * 0.9, 26 * dialogs.theme.rowHeight + 60)
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
            Item {
                id: rowsArea
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.minimumHeight: 3 * dialogs.theme.rowHeight
                Layout.preferredHeight: 16 * dialogs.theme.rowHeight
                implicitWidth: 600
            ListView {
                id: rows
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                // Whole rows only.
                height: Math.floor(parent.height / dialogs.theme.rowHeight) * dialogs.theme.rowHeight
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
            }
            // F1: what the highlighted command does. Its room is always
            // reserved, so showing or hiding it does not move the window.
            Label {
                Layout.fillWidth: true
                Layout.topMargin: 4
                Layout.preferredHeight: dialogs.view.help === "" ? dialogs.theme.rowHeight
                    : Math.min(contentHeight, 3 * dialogs.theme.rowHeight)
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
            // The answer; a longer one scrolls with the cursor (the whole
            // answer of a place is written under the field).
            Rectangle {
                id: fieldBox
                Layout.fillWidth: true
                implicitHeight: dialogs.theme.rowHeight + 6
                color: palette.base
                border.color: palette.highlight
                clip: true
                // How far the text moves left so that the cursor stays in.
                readonly property real shift: Math.min(0, width - 14 - beforeCursor.advanceWidth)
                Text {
                    id: field
                    anchors.verticalCenter: parent.verticalCenter
                    x: 4 + fieldBox.shift
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
                    x: 4 + fieldBox.shift + beforeCursor.advanceWidth
                    anchors.verticalCenter: parent.verticalCenter
                    width: 2
                    height: dialogs.theme.rowHeight
                    color: palette.text
                }
            }
            // The whole place, wrapped, with room for three rows.
            Item {
                visible: dialogs.view.is_path
                Layout.fillWidth: true
                Layout.preferredHeight: visible ? 3 * dialogs.theme.rowHeight : 0
                clip: true
                Label {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    text: dialogs.view.text
                    wrapMode: Text.WrapAnywhere
                    textFormat: Text.PlainText
                    font: dialogs.theme.fixedFont
                    color: dialogs.theme.dim
                }
            }
            // Under the field: examples, what was understood, the new names.
            // The room is kept whatever is written, so the window stays put.
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: Math.max(dialogs.view.preview_rows * dialogs.theme.rowHeight,
                                                 previewLines.implicitHeight)
                clip: true
                ColumnLayout {
                    id: previewLines
                    anchors.left: parent.left
                    anchors.right: parent.right
                    spacing: 0
                    Lines { table: true; model: dialogs.view.preview }
                }
            }
        }
    }

    Component {
        id: confirm
        Box {
            title: dialogs.view.title
            hint: dialogs.view.hint
            // Text taller than the dialog scrolls (wheel), from its top.
            Flickable {
                id: confirmScroll
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.preferredHeight: confirmLines.implicitHeight
                contentWidth: width
                contentHeight: confirmLines.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar {}
                ColumnLayout {
                    id: confirmLines
                    width: confirmScroll.width
                    Lines { model: dialogs.view.lines }
                }
            }
        }
    }

    Component {
        id: choose
        Box {
            id: chooseBox
            title: dialogs.view.title
            hint: dialogs.view.footer
            FontMetrics { id: chooseMetrics; font: dialogs.theme.fixedFont }
            // Tables (disks, drives, disk usage) get the width they need, up
            // to the window, so their columns are not cut.
            readonly property real wanted: {
                var widest = 0, items = dialogs.view.items
                widest += 0 * chooseMetrics.height
                for (var i = 0; i < items.length; i++)
                    widest = Math.max(widest, chooseMetrics.advanceWidth(
                        typeof items[i] === "string" ? items[i] : items[i].text))
                return widest + 48
            }
            width: Math.min(maxWidth, Math.max(theme.width * 0.6, 560, wanted))
            CursorList {
                fixed: true
                keepsSize: dialogs.view.keeps_size
                cursor: dialogs.view.cursor
                model: dialogs.view.items
            }
        }
    }

    Component {
        id: message
        Box {
            id: messageBox
            title: dialogs.view.title
            hint: dialogs.view.hint
            // A log (the steps of an install, a connection test) keeps one
            // size while lines are added, and shows its end.
            readonly property bool log: dialogs.view.busy
            fixedHeight: log ? dialogs.height * 0.7 : 0
            // About: the logo above the text.
            Image {
                visible: !!dialogs.view.logo_url && dialogs.height >= 600
                Layout.alignment: Qt.AlignHCenter
                // Smaller in a short window, so the text keeps its room.
                readonly property real size: Math.min(128, dialogs.height * 0.16)
                Layout.preferredWidth: visible ? size : 0
                Layout.preferredHeight: visible ? size : 0
                source: dialogs.view.logo_url
                        ? (dialogs.theme.dark ? dialogs.view.logo_url : dialogs.view.logo_url_light)
                        : ""
                fillMode: Image.PreserveAspectFit
                smooth: true
                mipmap: true
            }
            // The text scrolls (wheel) when it is taller than the window.
            Flickable {
                id: messageScroll
                visible: !messageBox.log
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.preferredHeight: messageLines.implicitHeight
                contentWidth: width
                contentHeight: messageLines.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar {}
                ColumnLayout {
                    id: messageLines
                    width: messageScroll.width
                    Lines { model: messageBox.log ? [] : dialogs.view.lines }
                }
            }
            ListView {
                id: logLines
                visible: messageBox.log
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                interactive: false
                model: messageBox.log ? dialogs.view.lines : []
                onCountChanged: positionViewAtEnd()
                delegate: Label {
                    width: ListView.view.width
                    text: modelData
                    wrapMode: Text.WrapAnywhere
                    textFormat: Text.PlainText
                    font: dialogs.theme.fixedFont
                    color: modelData.indexOf("Failed") >= 0 || modelData.indexOf("! ") >= 0
                           || modelData.indexOf("✗") === 0 ? dialogs.theme.errorColor
                           : modelData.indexOf("✓") === 0 ? dialogs.theme.codeColor
                           : palette.windowText
                }
            }
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
            title: "Transfer queue"
            hint: dialogs.view.hint
            fixedHeight: dialogs.height * 0.8
            width: Math.min(dialogs.theme.width * 0.9, 1000)
            Label {
                visible: dialogs.view.rows.length === 0
                text: "No transfers"
                color: dialogs.theme.dim
            }
            // Newest first. Each transfer takes a few rows: the job, then
            // where from and to, then how the data travels; long text wraps.
            ListView {
                id: jobRows
                Layout.fillWidth: true
                Layout.fillHeight: true
                interactive: false
                clip: true
                model: dialogs.view.rows
                onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)
                currentIndex: dialogs.view.cursor
                delegate: Rectangle {
                    readonly property bool current: index === dialogs.view.cursor
                    readonly property color ink: current ? palette.highlightedText
                        : modelData.state === "failed" ? dialogs.theme.errorColor
                        : palette.windowText
                    width: ListView.view.width
                    height: jobText.implicitHeight + jobDetail.implicitHeight + 8
                    color: current ? palette.highlight : "transparent"
                    radius: 2
                    Label {
                        id: jobText
                        x: 6
                        y: 2
                        width: parent.width - 12
                        text: modelData.text
                        font: dialogs.theme.fixedFont
                        wrapMode: Text.WrapAnywhere
                        color: ink
                    }
                    Label {
                        id: jobDetail
                        x: 6
                        anchors.top: jobText.bottom
                        width: parent.width - 12
                        text: modelData.detail
                        wrapMode: Text.WrapAnywhere
                        color: current ? palette.highlightedText : dialogs.theme.dim
                    }
                }
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
            width: Math.min(dialogs.theme.width * 0.9, 1000)
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

    // A copy conflict waits for an answer before anything else. The paths
    // wrap, so the whole of them can be read.
    Box {
        anchors.centerIn: parent
        visible: dialogs.conflict !== null
        title: "File exists"
        hint: dialogs.conflict ? dialogs.conflict.hint : ""
        Label {
            Layout.fillWidth: true
            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
            text: dialogs.conflict ? dialogs.conflict.destination + " already exists." : ""
        }
        // What is there and what comes, in two aligned columns.
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 12
            rowSpacing: 2
            Label { text: "Existing:" }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: dialogs.conflict ? dialogs.conflict.existing : ""
            }
            Label { text: "New:" }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: dialogs.conflict ? dialogs.conflict.incoming : ""
            }
        }
    }
}
