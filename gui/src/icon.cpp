// The window icon: Qt's QIcon is not in cxx-qt-lib, so this one call is C++.

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
