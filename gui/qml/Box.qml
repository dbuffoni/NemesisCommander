// A dialog window drawn over the panels: a title and content. It never
// takes the keyboard: the application answers the keys and redraws it.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: box
    property string title: ""
    property string hint: ""
    // Set by a text that does not fit, for a key row that says so.
    property bool overflows: false
    property color hintColor: theme.dim
    default property alias content: column.data
    readonly property var theme: ApplicationWindow.window

    // A window keeps its size while its text changes (typing, a log that
    // grows, a list that is filtered): the width is a share of the window,
    // never taken from the content, so long paths wrap instead of making the
    // window wider. A key row that is longer wraps onto more lines, so it
    // is always shown whole (also with a large font). The height follows
    // the content only for windows whose content never changes; the others
    // ask for a `fixedHeight` (a share of the window). Whoever shows it
    // centers it.
    readonly property real maxWidth: theme.width * 0.9
    property real fixedHeight: 0
    width: Math.min(maxWidth, Math.max(theme.width * 0.6, 560))
    height: Math.min(theme.height * 0.9, fixedHeight > 0 ? fixedHeight
            : header.height + column.implicitHeight + footer.implicitHeight + 44)
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
        wrapMode: Text.Wrap
        maximumLineCount: 2
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
        // Each "Key: verb" part stays whole: a break falls between parts.
        text: box.hint.split("    ").map(function (part) {
            return part.replace(/ /g, "\u00a0")
        }).join("    ")
        visible: text !== ""
        wrapMode: Text.Wrap
        font.pointSize: box.theme.font.pointSize * 0.9
        color: box.hintColor
    }
}
