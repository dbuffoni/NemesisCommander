// The viewer (F3) and quick view. `view` is a ViewerView of the screen
// description: text or hex lines, a formatted document (lines of styled
// spans), or a picture (an image, a PDF page, a view of a 3D model) with
// its zoom. The application has already cut the lines to what fits, so
// this only draws them.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: viewer
    property var view: null
    // The quick view has no header of keys: it is half a window wide.
    property bool compact: false
    readonly property var theme: ApplicationWindow.window
    readonly property bool showsImage: view !== null && view.image_url !== null
                                       && view.image_url !== undefined
    readonly property bool isPicture: view !== null
                                      && ["image", "pdf", "model"].indexOf(view.mode) >= 0
    readonly property bool isDocument: view !== null && view.rich.length > 0

    // Text for StyledText: special characters escaped, spaces kept.
    function escaped(text) {
        return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
                   .replace(/ /g, "&nbsp;")
    }

    // One document line as StyledText.
    function styledLine(spans, level) {
        let out = ""
        for (const span of spans) {
            let text = escaped(span.text)
            const style = span.style.split(" ")
            if (style.indexOf("code") >= 0)
                text = "<font color=\"" + viewer.theme.codeColor + "\">" + text + "</font>"
            if (style.indexOf("link") >= 0)
                text = "<font color=\"" + viewer.theme.linkColor + "\"><u>" + text + "</u></font>"
            if (style.indexOf("removed") >= 0)
                text = "<font color=\"" + viewer.theme.removedColor + "\">" + text + "</font>"
            if (style.indexOf("mark") >= 0)
                text = "<font color=\"" + viewer.theme.dim + "\">" + text + "</font>"
            const tones = { keyword: viewer.theme.keywordColor, string: viewer.theme.stringColor,
                            number: viewer.theme.numberColor, comment: viewer.theme.commentColor,
                            name: viewer.theme.nameColor }
            for (const tone in tones) {
                if (style.indexOf(tone) >= 0)
                    text = "<font color=\"" + tones[tone] + "\">" + text + "</font>"
            }
            if (style.indexOf("comment") >= 0)
                text = "<i>" + text + "</i>"
            if (style.indexOf("italic") >= 0)
                text = "<i>" + text + "</i>"
            if (style.indexOf("bold") >= 0 || level > 0)
                text = "<b>" + text + "</b>"
            out += text
        }
        return out
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 4

        // Header: what is shown, and the keys of this mode.
        // The keys go under the title when both do not fit side by side.
        FontMetrics { id: titleMetrics; font.family: viewer.theme.font.family
            font.pointSize: viewer.theme.font.pointSize; font.bold: true }
        FontMetrics { id: keysMetrics; font: viewer.theme.font }
        GridLayout {
            Layout.fillWidth: true
            visible: !viewer.compact && viewer.view !== null
            columnSpacing: 12
            rowSpacing: 0
            readonly property bool stacked: viewer.view !== null
                && 0 * (titleMetrics.height + keysMetrics.height)
                   + titleMetrics.advanceWidth(viewer.view.title)
                   + keysMetrics.advanceWidth(viewer.view.keys) + 12 > viewer.width
            columns: stacked ? 1 : 2
            Label {
                Layout.fillWidth: true
                elide: Text.ElideMiddle
                font.bold: true
                text: viewer.view ? viewer.view.title : ""
            }
            Label {
                Layout.fillWidth: parent.stacked
                wrapMode: Text.Wrap
                text: viewer.view ? viewer.view.keys : ""
                color: viewer.theme.dim
            }
        }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            visible: !viewer.compact
            color: viewer.theme.lineColor
        }

        Item {
            id: content
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true

            // Text and hex lines. A line wider than the window shrinks the
            // text, down to 60%, before anything is cut.
            FontMetrics { id: lineMetrics; font: viewer.theme.fixedFont }
            readonly property real shrink: {
                if (!viewer.view || viewer.isPicture || viewer.isDocument)
                    return 1
                var widest = 0 * lineMetrics.height, lines = viewer.view.lines
                for (var i = 0; i < lines.length; i++)
                    widest = Math.max(widest, lineMetrics.advanceWidth(lines[i]))
                return widest <= width ? 1 : Math.max(0.6, width / widest)
            }
            Column {
                visible: !viewer.isPicture && !viewer.isDocument
                Repeater {
                    model: viewer.view && !viewer.isPicture && !viewer.isDocument
                           ? viewer.view.lines : []
                    delegate: Text {
                        // Source and data files come with colours.
                        readonly property bool tinted:
                            viewer.view.tinted.length > index
                        text: tinted ? viewer.styledLine(viewer.view.tinted[index].spans, 0)
                                     : modelData
                        font.family: viewer.theme.fixedFont.family
                        font.pointSize: viewer.theme.fixedFont.pointSize * content.shrink
                        color: palette.text
                        height: viewer.theme.rowHeight
                        width: content.width
                        elide: Text.ElideRight
                        verticalAlignment: Text.AlignVCenter
                        textFormat: tinted ? Text.StyledText : Text.PlainText
                    }
                }
            }

            // A formatted document: headings larger, rules as thin lines,
            // code on a shaded band, the rest in the fixed font so tables
            // line up.
            Column {
                width: parent.width
                visible: viewer.isDocument
                Repeater {
                    model: viewer.view && viewer.isDocument ? viewer.view.rich : []
                    delegate: Item {
                        width: content.width
                        readonly property int level: modelData.heading
                        height: modelData.rule ? 8 : Math.max(viewer.theme.rowHeight,
                                                                line.implicitHeight + 2)
                        Rectangle {
                            anchors.fill: parent
                            visible: modelData.code
                            color: viewer.theme.lineColor
                        }
                        Rectangle {
                            visible: modelData.rule
                            anchors.verticalCenter: parent.verticalCenter
                            width: parent.width
                            height: 1
                            color: viewer.theme.lineColor
                        }
                        Text {
                            id: line
                            visible: !modelData.rule
                            anchors.verticalCenter: parent.verticalCenter
                            width: parent.width
                            wrapMode: modelData.code ? Text.NoWrap : Text.Wrap
                            text: viewer.styledLine(modelData.spans, parent.level)
                            textFormat: Text.StyledText
                            font.family: viewer.theme.fixedFont.family
                            font.pointSize: viewer.theme.fixedFont.pointSize
                                            * (parent.level === 1 ? 1.5 : parent.level === 2 ? 1.25 : 1)
                            color: parent.level > 0 && parent.level <= 2 ? viewer.theme.headingColor
                                                                         : palette.text
                        }
                    }
                }
            }

            // A picture, fitted to the space and then zoomed around `center`.
            Image {
                id: picture
                visible: viewer.showsImage && status === Image.Ready
                source: viewer.showsImage ? viewer.view.image_url : ""
                asynchronous: true
                cache: false
                smooth: true
                mipmap: true
                readonly property real fit: sourceSize.width > 0
                    ? Math.min(content.width / sourceSize.width,
                               content.height / sourceSize.height) : 1
                readonly property real zoom: viewer.view ? viewer.view.zoom : 1
                width: sourceSize.width * fit * zoom
                height: sourceSize.height * fit * zoom
                x: content.width / 2 - (viewer.view ? viewer.view.center[0] : 0.5) * width
                y: content.height / 2 - (viewer.view ? viewer.view.center[1] : 0.5) * height
            }
            // Pages get a thin frame so white paper stands out.
            Rectangle {
                visible: picture.visible && viewer.view.mode === "pdf"
                x: picture.x
                y: picture.y
                width: picture.width
                height: picture.height
                color: "transparent"
                border.color: viewer.theme.lineColor
            }

            Label {
                anchors.centerIn: parent
                width: Math.min(implicitWidth, parent.width - 16)
                wrapMode: Text.Wrap
                horizontalAlignment: Text.AlignHCenter
                visible: text !== ""
                text: {
                    const v = viewer.view
                    if (!v)
                        return ""
                    if (v.error !== null)
                        return v.error
                    if (v.loading)
                        return "Loading…"
                    if (viewer.isPicture && !viewer.showsImage)
                        return v.rendering ? "Drawing…" : "Picture (shown for files on this computer). F4: hex"
                    return ""
                }
                color: viewer.view && viewer.view.error !== null ? viewer.theme.errorColor
                                                                 : viewer.theme.dim
            }
        }

        // Footer: details of what is shown, and how far it goes.
        RowLayout {
            Layout.fillWidth: true
            spacing: 12
            Label {
                Layout.fillWidth: true
                elide: Text.ElideRight
                color: viewer.theme.dim
                text: !viewer.view ? "" : viewer.compact ? viewer.view.info || viewer.view.title
                                                         : viewer.view.info
            }
            Label {
                color: viewer.theme.dim
                visible: viewer.view !== null && !viewer.isPicture
                text: !viewer.view ? "" : (viewer.view.truncated ? "start of the file only    " : "")
                                          + (viewer.isDocument ? "" : viewer.view.mode + "  ")
                                          + viewer.view.percent + "%"
            }
        }
    }
}
