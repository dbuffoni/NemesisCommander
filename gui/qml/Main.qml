// NemesisCommander's window. It only draws: the state lives in the Rust
// `Commander` object, which publishes the whole screen as JSON (`screen`)
// and receives every key press. There are no toolbars or icons; commands
// are in the Commands list (F9) and on the keyboard (F1 lists the main
// shortcuts).

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import NemesisCommander

ApplicationWindow {
    // Losing the focus loses the releases: forget the held modifiers.
    onActiveChanged: if (!active) keys.hold(0)
    id: window
    width: commander.window_width
    height: commander.window_height
    visible: true
    title: screen ? "NemesisCommander — " + screen.panels[activeSide].title : "NemesisCommander"

    // The screen description (see nmc_frontend::view::Screen).
    readonly property var screen: commander.screen ? JSON.parse(commander.screen) : null
    // The application, for windows drawn in other files (clicks).
    readonly property var app: commander
    // The outlines of the file type icons, by shape name.
    readonly property var iconShapes: {
        var shapes = {}
        var list = screen ? screen.icons : []
        for (var i = 0; i < list.length; i++)
            shapes[list[i].id] = list[i].strokes
        return shapes
    }
    // Gives the keyboard back to the application (after a file picker).
    function focusKeys() { keys.forceActiveFocus() }
    readonly property int activeSide: screen && screen.panels[1].active ? 1 : 0

    // Shared sizes and colors.
    readonly property int rowHeight: Math.ceil(fixedMetrics.height * 1.3)
    // The fonts of the settings; empty or 0 keeps the system's.
    readonly property var fonts: screen ? screen.fonts : null
    font.family: fonts && fonts.family ? fonts.family : Qt.application.font.family
    font.pointSize: fonts && fonts.size > 0 ? fonts.size : Qt.application.font.pointSize
    readonly property font fixedFont: Qt.font({
        family: fonts && fonts.fixed_family ? fonts.fixed_family : "monospace",
        pointSize: font.pointSize
    })
    readonly property color accent: "#c8901a"
    readonly property color errorColor: "#c0392b"
    readonly property bool dark: palette.window.hslLightness < 0.5
    readonly property color headingColor: dark ? "#e0b050" : "#8a5a00"
    readonly property color codeColor: dark ? "#8fd18f" : "#1d7a33"
    readonly property color linkColor: dark ? "#7fb2ff" : "#1f5fbf"
    // Source code: the colours of syntax highlighting.
    readonly property color keywordColor: dark ? "#d69bf0" : "#8a2ea8"
    readonly property color stringColor: dark ? "#8fd18f" : "#1d7a33"
    readonly property color numberColor: dark ? "#f0c674" : "#a35f00"
    readonly property color commentColor: dark ? "#8a929c" : "#6b7480"
    readonly property color nameColor: dark ? "#6fd0e0" : "#0b6e80"
    readonly property color removedColor: dark ? "#ff7b72" : "#c62828"
    // The blue of the logo (#1F6BFF), transparent, marks the selected entries.
    readonly property color brandBlue: "#1F6BFF"
    readonly property color selectionTint: Qt.rgba(0x1F / 255, 0x6B / 255, 1.0, dark ? 0.32 : 0.22)
    readonly property color selectedText: dark ? "#8db6ff" : "#0b3fa8"
    readonly property color lineColor: Qt.rgba(palette.windowText.r, palette.windowText.g,
                                               palette.windowText.b, 0.18)
    readonly property color dim: Qt.rgba(palette.windowText.r, palette.windowText.g,
                                         palette.windowText.b, 0.6)

    FontMetrics {
        id: fixedMetrics
        font: window.fixedFont
    }

    // The system clipboard: Qt Quick has no direct call, but a text field
    // that selects and copies its text does the same.
    TextEdit {
        id: clipboardHelper
        visible: false
    }

    Commander {
        id: commander
        onRunningChanged: if (!running) Qt.quit()
        onPaste_seqChanged: keys.pasteClipboard()
        onClipboard_seqChanged: {
            clipboardHelper.text = commander.clipboard
            clipboardHelper.selectAll()
            clipboardHelper.copy()
        }
        // Self test: save a picture of the window, then quit.
        // Pictures load in the background, so give them a moment.
        onScreenshotChanged: shotTimer.start()
    }

    Timer {
        id: shotTimer
        interval: 400
        onTriggered: {
            const started = page.grabToImage((result) => {
                result.saveToFile(commander.screenshot)
                commander.quit()
            })
            if (!started) commander.quit()
        }
    }

    // The size of a normal window is kept for the next start.
    onClosing: if (visibility === ApplicationWindow.Windowed) commander.remember_window(width, height)

    Component.onCompleted: commander.start(Qt.fontFamilies().join("\n"))

    // Rows that fit: tell the application, which scrolls and pages with it.
    // The settings list shrinks its text (down to 60%) in a small window so
    // that more of its rows are in sight.
    readonly property bool settingsOpen: screen !== null && screen.overlay !== null
        && screen.overlay.kind === "settings"
    readonly property real settingsScale: !settingsOpen ? 1
        : Math.max(0.6, Math.min(1, body.height / (rowHeight * 16)))
    onSettingsScaleChanged: reportSize()
    function reportSize() {
        var unit = rowHeight * settingsScale
        commander.resize(left.visibleRows, Math.max(1, Math.floor(body.height / unit) - 4),
                         Math.floor(body.width / fixedMetrics.averageCharacterWidth) - 4)
    }

    // The whole window.
    Rectangle {
        id: page
        anchors.fill: parent
        color: palette.window

        // Every key goes to the application, Tab included.
        Item {
            id: keys
            anchors.fill: parent
            focus: true
            // Ctrl, Alt and Shift held right now (the function key bar shows
            // what they do with F1-F10). A modifier's own press and release
            // carry it in `modifiers` only on some platforms, so add or drop
            // it by its key.
            property int held: 0
            function bit(key) {
                return key === Qt.Key_Shift ? Qt.ShiftModifier
                     : key === Qt.Key_Control ? Qt.ControlModifier
                     : key === Qt.Key_Alt ? Qt.AltModifier : 0
            }
            function hold(mask) {
                if (mask !== held) {
                    held = mask
                    commander.modifiers(mask)
                }
            }
            // Ctrl+V and Shift+Insert paste the system clipboard, which is
            // read through the same hidden text field that copies.
            function pasteClipboard() {
                clipboardHelper.text = ""
                clipboardHelper.paste()
                commander.paste(clipboardHelper.text)
                clipboardHelper.text = ""
            }
            Keys.onPressed: (event) => {
                var own = bit(event.key)
                if ((event.key === Qt.Key_V && event.modifiers === Qt.ControlModifier)
                        || (event.key === Qt.Key_Insert && event.modifiers === Qt.ShiftModifier)) {
                    pasteClipboard()
                } else if (own !== 0) {
                    hold(event.modifiers | own)
                } else {
                    hold(event.modifiers)
                    commander.key(event.key, event.text, event.modifiers)
                }
                event.accepted = true
            }
            Keys.onReleased: (event) => {
                var own = bit(event.key)
                if (own !== 0)
                    hold(event.modifiers & ~own)
                else
                    hold(event.modifiers)
                event.accepted = true
            }
        }

        ColumnLayout {
            id: body
            anchors.fill: parent
            anchors.margins: 4
            spacing: 4
            onHeightChanged: window.reportSize()
            onWidthChanged: window.reportSize()

            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 4
                PanelView {
                    id: left
                    view: window.screen ? window.screen.panels[0] : null
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.preferredWidth: 1
                    onVisibleRowsChanged: window.reportSize()
                }
                PanelView {
                    side: 1
                    view: window.screen ? window.screen.panels[1] : null
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.preferredWidth: 1
                }
            }

            // Status line: a message, or details of the entry under the cursor.
            RowLayout {
                Layout.fillWidth: true
                Label {
                    Layout.fillWidth: true
                    // Whole words on two lines at most; an error keeps its
                    // beginning (the host), the end is cut.
                    wrapMode: Text.Wrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                    text: window.screen ? window.screen.status.text : ""
                    color: window.screen && window.screen.status.error ? window.errorColor
                                                                       : palette.windowText
                    font.bold: window.screen ? window.screen.status.message : false
                }
                Label {
                    text: window.screen ? window.screen.jobs : ""
                    color: window.accent
                }
            }

            // Progress of the transfers: a thin line, only while there are some.
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 3
                visible: window.screen ? window.screen.progress >= 0 : false
                color: palette.mid
                Rectangle {
                    height: parent.height
                    width: parent.width * (window.screen ? Math.max(0, window.screen.progress) : 0)
                    color: window.accent
                }
                // A click opens the transfer queue.
                MouseArea {
                    anchors.fill: parent
                    anchors.topMargin: -6
                    anchors.bottomMargin: -6
                    onClicked: commander.perform("tools.jobs")
                }
            }

            // Function keys F1-F10. When the window is too narrow for every
            // label, the least important keys show only their number (words
            // are never cut): F10 and F9 keep their label longest.
            FontMetrics { id: keyBold; font.family: window.font.family
                font.pointSize: window.font.pointSize; font.bold: true }
            FontMetrics { id: keyPlain; font: window.font }
            readonly property var keyList: window.screen ? window.screen.keys : []
            readonly property var keyPriority: [10, 9, 1, 3, 5, 6, 7, 8, 4, 2]
            function fullWidth(key, labelled) {
                // (the metrics change with the font: depend on them)
                return 8 + 0 * (keyBold.height + keyPlain.height) + keyBold.advanceWidth("F" + key.number)
                     + (labelled ? 4 + keyPlain.advanceWidth(key.label) : 0)
            }
            // The text shrinks, down to 70%, so that every label fits.
            readonly property real keyScale: {
                var all = 0, gaps = 2 * keyList.length
                for (var i = 0; i < keyList.length; i++)
                    all += fullWidth(keyList[i], true)
                return all + gaps <= body.width ? 1
                    : Math.max(0.4, (body.width - gaps - 4) / all)
            }
            function keyWidth(key, labelled) {
                return fullWidth(key, labelled) * keyScale
            }
            // The numbers of the keys that keep their label.
            readonly property var labelled: {
                var avail = body.width - 2 * keyList.length, used = 0, i
                for (i = 0; i < keyList.length; i++)
                    used += keyWidth(keyList[i], false)
                var shown = {}
                for (var p = 0; p < keyPriority.length; p++) {
                    for (i = 0; i < keyList.length; i++) {
                        var k = keyList[i]
                        if (k.number !== keyPriority[p])
                            continue
                        var extra = keyWidth(k, true) - keyWidth(k, false)
                        if (used + extra <= avail) {
                            used += extra
                            shown[k.number] = true
                        }
                    }
                }
                return shown
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 2
                Repeater {
                    model: window.screen ? window.screen.keys : []
                    delegate: Rectangle {
                        readonly property bool withLabel: !!body.labelled[modelData.number]
                        Layout.fillWidth: true
                        Layout.preferredWidth: body.keyWidth(modelData, withLabel)
                        Layout.minimumWidth: body.keyWidth(modelData, false)
                        implicitHeight: window.rowHeight
                        color: palette.button
                        border.color: window.lineColor
                        radius: 2
                        // A click is the same as the key: F9 opens the
                        // Commands list.
                        MouseArea {
                            anchors.fill: parent
                            onClicked: {
                                commander.key(Qt.Key_F1 + modelData.number - 1, "", keys.held)
                                keys.forceActiveFocus()
                            }
                        }
                        Row {
                            anchors.verticalCenter: parent.verticalCenter
                            x: 4
                            spacing: 4
                            Label {
                                text: "F" + modelData.number
                                font.bold: true
                                font.pointSize: window.font.pointSize * body.keyScale
                            }
                            Label {
                                visible: withLabel
                                text: modelData.label
                                color: palette.buttonText
                                font.pointSize: window.font.pointSize * body.keyScale
                            }
                        }
                    }
                }
            }
        }

        DialogView {
            anchors.fill: parent
            view: window.screen ? window.screen.overlay : null
            conflict: window.screen ? window.screen.conflict : null
        }
    }
}
