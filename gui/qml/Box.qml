// A dialog window drawn over the panels: a title and content. It never
// takes the keyboard: the application answers the keys and redraws it.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: box
    property string title: ""
    property string hint: ""
    property color hintColor: theme.dim
    default property alias content: column.data
    readonly property var theme: ApplicationWindow.window

    // Sized by its content, up to 90% of the window; a key row that is still
    // longer wraps onto more lines, so it is always shown whole (also with a
    // large font). Whoever shows it centers it.
    readonly property real maxWidth: theme.width * 0.9
    width: Math.min(maxWidth, Math.max(420, column.implicitWidth + 32,
                                       footer.implicitWidth + 32))
    height: Math.min(theme.height * 0.9,
                     header.height + column.implicitHeight + footer.implicitHeight + 44)
    color: palette.window
    border.color: palette.highlight
    border.width: 2
    radius: 4

    Label {
        id: header
        x: 16
        y: 10
        width: parent.width - 32
        text: box.title
        font.bold: true
        elide: Text.ElideRight
    }

    ColumnLayout {
        id: column
        x: 16
        anchors.top: header.bottom
        anchors.topMargin: 10
        anchors.bottom: footer.visible ? footer.top : parent.bottom
        anchors.bottomMargin: 8
        width: parent.width - 32
        spacing: 4
        clip: true
    }

    Label {
        id: footer
        x: 16
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 8
        width: parent.width - 32
        text: box.hint
        visible: text !== ""
        wrapMode: Text.Wrap
        color: box.hintColor
    }
}
