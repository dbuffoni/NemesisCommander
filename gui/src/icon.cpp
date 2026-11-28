// The window icon: Qt's QIcon is not in cxx-qt-lib, so this one call is C++.

#include <QtCore/QByteArray>
#include <QtCore/QtGlobal>
#include <QtGui/QGuiApplication>
#include <QtGui/QIcon>
#include <QtGui/QPixmap>

extern "C" void pnxr_set_window_icon(const unsigned char *data, unsigned long long length)
{
    // Desktops match the window to its launcher (and icon) by this name.
    QGuiApplication::setDesktopFileName(QStringLiteral("pairnox"));
    // Each call adds one size: the title bar, the taskbar and the task
    // switcher each pick the one that fits them.
    static QIcon icon;
    QPixmap picture;
    if (picture.loadFromData(data, static_cast<uint>(length))) {
        icon.addPixmap(picture);
        QGuiApplication::setWindowIcon(icon);
    }
}

// On Windows the default Quick Controls style gives the window the light
// palette and its children the dark one, so text drawn with the window's
// colours stays black on a dark window. Fusion has one palette for both. The
// style is read when the first window is loaded; a choice made by the
// user (QT_QUICK_CONTROLS_STYLE) is kept.
extern "C" void pnxr_use_consistent_style()
{
#ifdef Q_OS_WIN
    if (!qEnvironmentVariableIsSet("QT_QUICK_CONTROLS_STYLE"))
        qputenv("QT_QUICK_CONTROLS_STYLE", "Fusion");
    // Qt's Direct3D vblank thread can fail to deliver update requests: only
    // the first frame is drawn and the layout is not updated (a panel shows
    // one row) until the window is resized. Timer-based updates avoid it.
    if (!qEnvironmentVariableIsSet("QT_D3D_NO_VBLANK_THREAD"))
        qputenv("QT_D3D_NO_VBLANK_THREAD", "1");
#endif
}
