//! The declaration of the `Commander` QML object for cxx-qt, which
//! generates the C++ glue from it. The glue is `unsafe` code by nature, so
//! this module alone allows it; the implementation is in [`crate::bridge`].

use crate::bridge::CommanderRust;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// Qt's string type.
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        /// The application, as seen from QML.
        #[qobject]
        #[qml_element]
        #[qproperty(QString, screen)]
        #[qproperty(QString, menus)]
        #[qproperty(bool, running)]
        #[qproperty(QString, screenshot)]
        type Commander = super::CommanderRust;

        /// Creates the application; call once, when the window is ready.
        #[qinvokable]
        fn start(self: Pin<&mut Commander>);

        /// A key press: `event.key`, `event.text` and `event.modifiers`.
        #[qinvokable]
        fn key(self: Pin<&mut Commander>, key: i32, text: &QString, modifiers: i32);

        /// A menu command, by action id.
        #[qinvokable]
        fn perform(self: Pin<&mut Commander>, id: &QString);

        /// Ends the application.
        #[qinvokable]
        fn quit(self: Pin<&mut Commander>);

        /// How many rows fit in a panel and in the viewer.
        #[qinvokable]
        fn resize(self: Pin<&mut Commander>, rows: i32, viewer_rows: i32);
    }

    impl cxx_qt::Threading for Commander {}
}
