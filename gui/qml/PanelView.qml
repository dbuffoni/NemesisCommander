// One side of the window: tabs, location, the visible rows of the listing
// and a footer, or the quick view. `view` is a PanelView of the screen
// description.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: panel
    property var view: null
    // Rows that fit in the listing; the window reports it to the application.
    readonly property int visibleRows: Math.max(1, Math.floor(list.height / theme.rowHeight))
    readonly property var theme: ApplicationWindow.window

    color: palette.base
    border.width: view && view.active ? 2 : 1
    border.color: view && view.active ? palette.highlight : palette.mid
    radius: 2

    TextMetrics {
        id: sizeWidth
        font: panel.theme.fixedFont
        text: "1023.9 MiB"
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
                        color: modelData.active ? palette.highlightedText : palette.buttonText
                    }
                }
            }
        }

        Label {
            Layout.fillWidth: true
            text: panel.view ? panel.view.title : ""
            elide: Text.ElideMiddle
            font.bold: true
        }

        // The listing.
        Item {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: panel.view !== null && !panel.view.quick_view

            Column {
                width: parent.width
                Repeater {
                    model: panel.view ? panel.view.rows : []
                    delegate: Rectangle {
                        id: row
                        width: list.width
                        height: panel.theme.rowHeight
                        color: modelData.cursor && panel.view.active ? palette.highlight
                             : modelData.cursor ? palette.midlight
                             : "transparent"
                        readonly property color textColor:
                            modelData.cursor && panel.view.active ? palette.highlightedText
                            : modelData.selected ? panel.theme.accent
                            : modelData.hidden ? panel.theme.dim
                            : palette.text
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 4
                            anchors.rightMargin: 4
                            spacing: 12
                            Label {
                                Layout.fillWidth: true
                                text: modelData.name
                                elide: Text.ElideRight
                                color: row.textColor
                                font.bold: modelData.dir || modelData.selected
                            }
                            Label {
                                text: modelData.size
                                horizontalAlignment: Text.AlignRight
                                Layout.preferredWidth: sizeWidth.width
                                color: row.textColor
                                font: panel.theme.fixedFont
                            }
                            Label {
                                text: modelData.date
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
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: panel.view !== null && !!panel.view.quick_view
            view: panel.view ? panel.view.quick_view : null
        }

        Label {
            Layout.fillWidth: true
            text: panel.view ? panel.view.footer : ""
            color: panel.theme.dim
            elide: Text.ElideRight
        }
    }
}
