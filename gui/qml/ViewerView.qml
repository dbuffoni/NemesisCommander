// The viewer (F3) and quick view: text or hex lines, or a picture. `view`
// is a ViewerView of the screen description; the application has already
// cut the lines to what fits, so this only draws them.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: viewer
    property var view: null
    readonly property var theme: ApplicationWindow.window
    readonly property bool showsImage: view !== null && view.mode === "image"

    ColumnLayout {
        anchors.fill: parent
        spacing: 2

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true

            Column {
                visible: !viewer.showsImage
                Repeater {
                    model: viewer.view && !viewer.showsImage ? viewer.view.lines : []
                    delegate: Text {
                        text: modelData
                        font: viewer.theme.fixedFont
                        color: palette.text
                        height: viewer.theme.rowHeight
                        verticalAlignment: Text.AlignVCenter
                        textFormat: Text.PlainText
                    }
                }
            }

            // Pictures of the local disk are shown at full quality; others
            // only have a description.
            Image {
                anchors.fill: parent
                visible: viewer.showsImage && source != ""
                source: viewer.showsImage && viewer.view.image_url ? viewer.view.image_url : ""
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                smooth: true
            }
            Label {
                anchors.centerIn: parent
                visible: viewer.showsImage && !viewer.view.image_url
                text: "Picture (only shown for files on this computer). F4: hex"
                color: viewer.theme.dim
            }

            Label {
                anchors.centerIn: parent
                visible: viewer.view !== null && (viewer.view.loading || viewer.view.error !== null)
                text: !viewer.view ? "" : viewer.view.error !== null ? viewer.view.error
                                                                     : "Loading…"
                color: viewer.view && viewer.view.error !== null ? viewer.theme.errorColor
                                                                 : viewer.theme.dim
            }
        }

        Label {
            Layout.fillWidth: true
            color: viewer.theme.dim
            elide: Text.ElideMiddle
            text: !viewer.view ? "" : viewer.view.title + "    " + viewer.view.mode
                  + "    " + viewer.view.percent + "%"
                  + (viewer.view.truncated ? "    (start of the file only)" : "")
        }
    }
}
