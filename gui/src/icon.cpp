// The window icon: Qt's QIcon is not in cxx-qt-lib, so this one call is C++.

#include <QtGui/QGuiApplication>
#include <QtGui/QIcon>
#include <QtGui/QPixmap>

extern "C" void nmc_set_window_icon(const unsigned char *data, unsigned long long length)
{
    // Desktops match the window to its launcher (and icon) by this name.
    QGuiApplication::setDesktopFileName(QStringLiteral("nemesiscommander"));
    QPixmap picture;
    if (picture.loadFromData(data, static_cast<uint>(length)))
        QGuiApplication::setWindowIcon(QIcon(picture));
}
