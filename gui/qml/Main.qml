// SuperCommander's window. It only draws: the state lives in the Rust
// `Commander` object, which publishes the whole screen as JSON (`screen`)
// and receives every key press. There are no toolbars or icons; commands
// are in the menu bar and on the keyboard (F1 lists the main shortcuts).

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import SuperCommander

ApplicationWindow {
    id: window
    width: commander.window_width
    height: commander.window_height
    visible: true
    title: screen ? "SuperCommander — " + screen.panels[activeSide].title : "SuperCommander"

    // The screen description (see sc_frontend::view::Screen).
    readonly property var screen: commander.screen ? JSON.parse(commander.screen) : null
    readonly property var menuList: commander.menus ? JSON.parse(commander.menus) : []
    // The application, for windows drawn in other files (clicks).
    readonly property var app: commander
    // Gives the keyboard back to the application (after a file picker).
    function focusKeys() { keys.forceActiveFocus() }
    readonly property int activeSide: screen && screen.panels[1].active ? 1 : 0

    // Shared sizes and colors.
    readonly property int rowHeight: Math.ceil(fixedMetrics.height * 1.3)
    readonly property font fixedFont: Qt.font({ family: "monospace", pointSize: font.pointSize })
    readonly property color accent: "#c8901a"
    readonly property color errorColor: "#c0392b"
    readonly property bool dark: palette.window.hslLightness < 0.5
    readonly property color headingColor: dark ? "#e0b050" : "#8a5a00"
    readonly property color codeColor: dark ? "#8fd18f" : "#1d7a33"
    readonly property color linkColor: dark ? "#7fb2ff" : "#1f5fbf"
    readonly property color selectionTint: Qt.rgba(0.78, 0.56, 0.10, dark ? 0.22 : 0.16)
    readonly property color selectedText: dark ? "#f0c060" : "#7a4d00"
    readonly property color lineColor: Qt.rgba(palette.windowText.r, palette.windowText.g,
                                               palette.windowText.b, 0.18)
    readonly property color dim: Qt.rgba(palette.windowText.r, palette.windowText.g,
                                         palette.windowText.b, 0.6)

    FontMetrics {
        id: fixedMetrics
        font: window.fixedFont
    }

    Commander {
        id: commander
        onRunningChanged: if (!running) Qt.quit()
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

    Component.onCompleted: commander.start()

    // Rows that fit: tell the application, which scrolls and pages with it.
    function reportSize() {
        commander.resize(left.visibleRows, Math.max(1, Math.floor(body.height / rowHeight) - 4),
                         Math.floor(body.width / fixedMetrics.averageCharacterWidth) - 4)
    }

    // Menus built from the command table, each item with its shortcut. The
    // keyboard focus goes back to the panels when a menu closes.
    menuBar: MenuBar {
        id: bar
        Instantiator {
            model: window.menuList
            delegate: Menu {
                id: menu
                title: modelData.title
                onClosed: keys.forceActiveFocus()
                Instantiator {
                    model: modelData.items
                    delegate: MenuItem {
                        text: modelData.label
                        onTriggered: commander.perform(modelData.id)
                        contentItem: RowLayout {
                            spacing: 32
                            Label {
                                text: modelData.label
                                Layout.fillWidth: true
                            }
                            Label {
                                text: modelData.shortcut
                                color: window.dim
                            }
                        }
                    }
                    onObjectAdded: (index, object) => menu.insertItem(index, object)
                    onObjectRemoved: (index, object) => menu.removeItem(object)
                }
            }
            onObjectAdded: (index, object) => bar.insertMenu(index, object)
            onObjectRemoved: (index, object) => bar.removeMenu(object)
        }
    }

    // Everything below the menu bar.
    Rectangle {
        id: page
        anchors.fill: parent
        color: palette.window

        // Every key goes to the application, Tab included.
        Item {
            id: keys
            anchors.fill: parent
            focus: true
            Keys.onPressed: (event) => {
                commander.key(event.key, event.text, event.modifiers)
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

            // Function keys F1-F10.
            RowLayout {
                Layout.fillWidth: true
                spacing: 2
                Repeater {
                    model: window.screen ? window.screen.keys : []
                    delegate: Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredWidth: 1
                        implicitHeight: window.rowHeight
                        color: palette.button
                        radius: 2
                        Row {
                            anchors.verticalCenter: parent.verticalCenter
                            x: 4
                            spacing: 4
                            Label {
                                text: "F" + modelData.number
                                font.bold: true
                            }
                            Label {
                                text: modelData.label
                                color: palette.buttonText
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
