// One side of the window: tabs, location, the visible rows of the listing
// and a footer, or the quick view. `view` is a PanelView of the screen
// description.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: panel
    property var view: null
    // 0 for the left panel, 1 for the right one.
    property int side: 0
    // Rows that fit in the listing; the window reports it to the application.
    readonly property int visibleRows: Math.max(1, Math.floor(list.height / theme.rowHeight))
    readonly property var theme: ApplicationWindow.window

    color: palette.base
    border.width: view && view.active ? 2 : 1
    border.color: view && view.active ? palette.highlight : palette.mid
    radius: 2

    // The name keeps at least a dozen characters; the size, then the date,
    // are left out when the panel is too narrow for them.
    TextMetrics {
        id: nameMin
        font: panel.theme.font
        text: "xxxxxxxxxxxx"
    }
    readonly property real iconSize: Math.max(16, Math.round(theme.rowHeight * 0.8))
    readonly property real fixedRoom: width - 8 - 8 - nameMin.width - iconSize - 12
    readonly property bool showSize: fixedRoom >= sizeWidth.width + 12
    readonly property bool showDate: showSize && fixedRoom >= sizeWidth.width + dateWidth.width + 24
    readonly property bool showFolder: view !== null && view.flat && showDate
        && fixedRoom >= sizeWidth.width + dateWidth.width + 150 + 36

    // Plugin columns come out of the name column; as many as fit are shown.
    readonly property var pluginHeads: view ? view.plugin_heads : []
    readonly property real charWidth: sizeWidth.width / 10
    readonly property int pluginCount: {
        var room = fixedRoom - (showSize ? sizeWidth.width + 12 : 0)
                   - (showDate ? dateWidth.width + 12 : 0) - (showFolder ? 162 : 0)
        var n = 0
        for (var i = 0; i < pluginHeads.length; i++) {
            var w = pluginHeads[i].width * charWidth + 12
            if (w > room)
                break
            room -= w
            n++
        }
        return n
    }

    TextMetrics {
        id: sizeWidth
        font: panel.theme.fixedFont
        text: "1023.9 MiB"
    }
    TextMetrics {
        id: dateWidth
        font: panel.theme.fixedFont
        text: "2026-12-31 23:59"
    }

    readonly property bool hasFolderText: view !== null && !view.quick_view
                                          && view.folder_text.length > 0

    // The folder text as one StyledText: the spans are styled as the viewer
    // styles them, single spaces let a paragraph wrap, and the line under a
    // heading is left out (the heading's colour marks it).
    function folderMarkup(lines) {
        let out = []
        let heading = false
        for (const line of lines) {
            if (line.rule) {
                if (!heading)
                    out.push("\u2500\u2500\u2500")
                heading = false
                continue
            }
            heading = line.heading > 0
            let text = quickView.styledLine(line.spans, line.heading)
            if (!line.code)
                text = text.replace(/&nbsp;(?!&nbsp;)/g, " ")
            if (heading)
                text = "<font color=\"" + panel.theme.headingColor + "\">" + text + "</font>"
            out.push(text)
        }
        return out.join("<br>")
    }

    // Files dropped here, from the other panel or from another program, are
    // offered for copying into this folder.
    DropArea {
        anchors.fill: parent
        onDropped: (drop) => {
            if (drop.formats.indexOf("application/x-pairnox") >= 0) {
                const part = drop.getDataAsString("application/x-pairnox").split(":")
                panel.theme.app.drag_rows(parseInt(part[0]), parseInt(part[1]), panel.side)
            } else if (drop.hasUrls) {
                panel.theme.app.drop_files(panel.side, drop.urls.join("\n"))
            }
            drop.accept(Qt.CopyAction)
            panel.theme.focusKeys()
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 4
        spacing: 2

        // Tabs, when there is more than one.
        Row {
            visible: panel.view !== null && panel.view.tabs.length > 1
            spacing: 2
            Repeater {
                model: panel.view ? panel.view.tabs : []
                delegate: Rectangle {
                    width: tabLabel.implicitWidth + 12
                    height: tabLabel.implicitHeight + 4
                    radius: 2
                    color: modelData.active ? palette.highlight : palette.button
                    Label {
                        id: tabLabel
                        anchors.centerIn: parent
                        text: modelData.label
                        color: modelData.active ? palette.highlightedText : panel.theme.buttonInk
                    }
                }
            }
        }

        // The drive or connection of this panel; a click chooses another
        // one (the same list as Shift+F1).
        RowLayout {
            Layout.fillWidth: true
            spacing: 6
            Rectangle {
                // The quick view is on no drive.
                visible: panel.view !== null && panel.view.drive !== ""
                implicitWidth: driveLabel.implicitWidth + 12
                implicitHeight: driveLabel.implicitHeight + 4
                radius: 2
                color: palette.button
                Label {
                    id: driveLabel
                    anchors.centerIn: parent
                    text: (panel.view ? panel.view.drive : "") + " \u25be"
                    color: panel.theme.buttonInk
                }
                MouseArea {
                    anchors.fill: parent
                    onClicked: {
                        panel.theme.app.pick("drive", panel.side)
                        panel.theme.focusKeys()
                    }
                }
            }
            Label {
                Layout.fillWidth: true
                text: panel.view ? panel.view.title : ""
                elide: Text.ElideMiddle
                font.bold: true
            }
            Label {
                visible: panel.view !== null && panel.view.branch !== ""
                text: panel.view ? "\u2387 " + panel.view.branch : ""
                Layout.maximumWidth: panel.width * 0.4
                elide: Text.ElideRight
                color: panel.theme.accent
            }
        }

        // The text of pnxr_folder.md, at most a third of the panel.
        Text {
            Layout.fillWidth: true
            Layout.maximumHeight: panel.height / 3
            maximumLineCount: Math.max(1, Math.floor(panel.height / 3 / nameMin.height))
            Layout.leftMargin: 4
            Layout.rightMargin: 4
            visible: panel.hasFolderText
            text: panel.hasFolderText ? panel.folderMarkup(panel.view.folder_text) : ""
            textFormat: Text.StyledText
            font: panel.theme.font
            color: panel.theme.ink
            wrapMode: Text.Wrap
            elide: Text.ElideRight
            clip: true
        }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            color: panel.theme.lineColor
            visible: panel.hasFolderText
        }

        // Column titles; the sorted one has an arrow.
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 4
            Layout.rightMargin: 4
            spacing: 12
            visible: panel.view !== null && !panel.view.quick_view
            Label {
                Layout.fillWidth: true
                elide: Text.ElideRight
                text: panel.view ? panel.view.headers[0] : ""
                color: panel.theme.dim
                font.pointSize: panel.theme.font.pointSize * 0.9
            }
            Label {
                visible: panel.showFolder
                Layout.preferredWidth: 150
                text: "Folder"
                color: panel.theme.dim
                font.pointSize: panel.theme.font.pointSize * 0.9
            }
            Repeater {
                model: panel.pluginCount
                delegate: Label {
                    Layout.preferredWidth: panel.pluginHeads[index].width * panel.charWidth
                    text: panel.pluginHeads[index].title
                    elide: Text.ElideRight
                    color: panel.theme.dim
                    font.pointSize: panel.theme.font.pointSize * 0.9
                }
            }
            Label {
                visible: panel.showSize
                Layout.preferredWidth: sizeWidth.width
                horizontalAlignment: Text.AlignRight
                text: panel.view ? panel.view.headers[1] : ""
                color: panel.theme.dim
                font.pointSize: panel.theme.font.pointSize * 0.9
            }
            Label {
                visible: panel.showDate
                Layout.preferredWidth: dateWidth.width
                text: panel.view ? panel.view.headers[2] : ""
                color: panel.theme.dim
                font.pointSize: panel.theme.font.pointSize * 0.9
            }
        }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            color: panel.theme.lineColor
            visible: panel.view !== null && !panel.view.quick_view
        }

        // The listing.
        Item {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: panel.view !== null && !panel.view.quick_view
            readonly property var blank: ({ cursor: false, selected: false, missing: false, hidden: false,
                dir: false, name: "", folder: "", size: "", date: "", cells: [], git: "", icon: "", icon_color: "",
                url: "" })

            WheelHandler {
                onWheel: (event) => panel.theme.app.scroll_rows(panel.side,
                                                                event.angleDelta.y > 0 ? -3 : 3)
            }

            Column {
                width: parent.width
                Repeater {
                    // A count, not the rows: the delegates stay where they are
                    // and only their contents change, so moving the cursor does
                    // not rebuild (and flash) every row.
                    model: panel.view ? panel.view.rows.length : 0
                    delegate: Rectangle {
                        id: row
                        readonly property var entry:
                            panel.view && index < panel.view.rows.length ? panel.view.rows[index]
                                                                         : list.blank
                        width: list.width
                        height: panel.theme.rowHeight
                        // The cursor is a bar (strong in the active panel, faint in
                        // the other); selected entries are tinted and bold.
                        readonly property bool strong: entry.cursor && panel.view.active
                        // Drag a row to the other panel, or out to another
                        // program (local files only), and click or double
                        // click it.
                        Drag.active: rowDrag.active
                        Drag.dragType: Drag.Automatic
                        Drag.supportedActions: Qt.CopyAction
                        Drag.mimeData: entry.url !== ""
                            ? { "application/x-pairnox": panel.side + ":" + index,
                                "text/uri-list": entry.url }
                            : { "application/x-pairnox": panel.side + ":" + index }
                        DragHandler {
                            id: rowDrag
                            target: null
                            acceptedButtons: Qt.LeftButton
                        }
                        TapHandler {
                            onTapped: {
                                panel.theme.app.click_row(panel.side, index)
                                panel.theme.focusKeys()
                            }
                            onDoubleTapped: {
                                panel.theme.app.click_row(panel.side, index)
                                panel.theme.app.perform("nav.open")
                                panel.theme.focusKeys()
                            }
                        }
                        color: strong ? palette.highlight
                             : entry.cursor ? panel.theme.lineColor
                             : entry.missing ? panel.theme.missingTint
                             : entry.selected ? panel.theme.selectionTint
                             : "transparent"
                        readonly property color textColor:
                            strong ? palette.highlightedText
                            : entry.missing ? panel.theme.missingText
                            : entry.selected ? panel.theme.selectedText
                            : entry.hidden ? panel.theme.dim
                            : palette.text
                        // A mark at the left edge keeps selection visible under the cursor.
                        Rectangle {
                            visible: entry.selected
                            width: 3
                            height: parent.height
                            color: entry.missing ? panel.theme.missingText : panel.theme.brandBlue
                        }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 4
                            anchors.rightMargin: 4
                            spacing: 12
                            // Git: M changed, ? untracked, ! ignored.
                            Label {
                                visible: panel.view.branch !== ""
                                Layout.preferredWidth: panel.theme.rowHeight * 0.6
                                horizontalAlignment: Text.AlignHCenter
                                text: entry.git
                                font: panel.theme.fixedFont
                                color: entry.git === "M" ? panel.theme.accent
                                     : entry.git === "?" ? panel.theme.codeColor
                                     : panel.theme.dim
                            }
                            // The kind of file, as a bold single-colour outline.
                            Canvas {
                                id: glyph
                                visible: entry.icon !== ""
                                Layout.preferredWidth: visible ? panel.iconSize : 0
                                Layout.preferredHeight: panel.iconSize
                                Layout.rightMargin: -6
                                Layout.alignment: Qt.AlignVCenter
                                property string shape: entry.icon
                                // On the cursor bar the icon takes the text colour,
                                // or blue folders vanish into the blue bar.
                                property string tint: row.strong ? String(palette.highlightedText)
                                                                 : entry.icon_color
                                onShapeChanged: requestPaint()
                                onTintChanged: requestPaint()
                                onWidthChanged: requestPaint()
                                onPaint: {
                                    var ctx = getContext("2d")
                                    ctx.reset()
                                    ctx.scale(width / 16, height / 16)
                                    var strokes = panel.theme.iconShapes[shape]
                                    if (!strokes)
                                        return
                                    ctx.strokeStyle = tint
                                    ctx.lineWidth = 1.6
                                    ctx.lineJoin = "round"
                                    ctx.lineCap = "round"
                                    for (var i = 0; i < strokes.length; i++) {
                                        var line = strokes[i]
                                        ctx.beginPath()
                                        ctx.moveTo(line[0], line[1])
                                        for (var j = 2; j + 1 < line.length; j += 2)
                                            ctx.lineTo(line[j], line[j + 1])
                                        ctx.stroke()
                                    }
                                }
                            }
                            Label {
                                Layout.fillWidth: true
                                text: entry.name
                                elide: Text.ElideRight
                                color: row.textColor
                                font.bold: entry.dir || entry.selected
                            }
                            // The flat view: where the file is, the deepest folder
                            // kept when the path is long.
                            Label {
                                visible: panel.showFolder
                                Layout.preferredWidth: 150
                                text: entry.folder
                                elide: Text.ElideLeft
                                color: panel.theme.dim
                            }
                            Repeater {
                                model: panel.pluginCount
                                delegate: Label {
                                    Layout.preferredWidth: panel.pluginHeads[index].width * panel.charWidth
                                    text: entry.cells && index < entry.cells.length ? entry.cells[index] : ""
                                    elide: Text.ElideRight
                                    color: row.textColor
                                    font: panel.theme.fixedFont
                                }
                            }
                            Label {
                                visible: panel.showSize
                                text: entry.size
                                horizontalAlignment: Text.AlignRight
                                Layout.preferredWidth: sizeWidth.width
                                color: row.textColor
                                font: panel.theme.fixedFont
                            }
                            Label {
                                visible: panel.showDate
                                text: entry.date
                                Layout.preferredWidth: dateWidth.width
                                color: row.textColor
                                font: panel.theme.fixedFont
                            }
                        }
                    }
                }
            }

            Label {
                anchors.centerIn: parent
                visible: panel.view !== null && (panel.view.loading || panel.view.error !== null)
                text: !panel.view ? "" : panel.view.error !== null ? panel.view.error : "Loading…"
                color: panel.view && panel.view.error !== null ? panel.theme.errorColor
                                                               : panel.theme.dim
                wrapMode: Text.Wrap
                width: parent.width - 16
                horizontalAlignment: Text.AlignHCenter
            }
        }

        // Quick view (Ctrl+Q) in place of the listing.
        ViewerView {
            id: quickView
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: panel.view !== null && !!panel.view.quick_view
            view: panel.view ? panel.view.quick_view : null
            compact: true
        }

        Label {
            Layout.fillWidth: true
            text: panel.view ? panel.view.footer : ""
            color: panel.theme.dim
            // Whole words, on two lines at most: the keys of the quick view
            // footer must not end in an ellipsis.
            wrapMode: Text.Wrap
            maximumLineCount: 3
            elide: Text.ElideRight
        }
    }
}
