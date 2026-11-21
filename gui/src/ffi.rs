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
        #[qproperty(bool, running)]
        #[qproperty(QString, clipboard)]
        #[qproperty(i32, clipboard_seq)]
        #[qproperty(i32, paste_seq)]
        #[qproperty(QString, screenshot)]
        #[qproperty(i32, window_width)]
        #[qproperty(i32, window_height)]
        type Commander = super::CommanderRust;

        /// Creates the application; call once, when the window is ready.
        /// `fonts` are the installed font families, one per line.
        #[qinvokable]
        fn start(self: Pin<&mut Commander>, fonts: &QString);

        /// A key press: `event.key`, `event.text` and `event.modifiers`.
        #[qinvokable]
        fn key(self: Pin<&mut Commander>, key: i32, text: &QString, modifiers: i32);

        /// Text pasted from the system clipboard (Ctrl+V).
        #[qinvokable]
        fn paste(self: Pin<&mut Commander>, text: &QString);

        /// Ctrl, Alt and Shift as held (`Qt::KeyboardModifier` bits): the
        /// function key bar shows what those combinations do.
        #[qinvokable]
        fn modifiers(self: Pin<&mut Commander>, modifiers: i32);

        /// A menu command, by command id.
        #[qinvokable]
        fn perform(self: Pin<&mut Commander>, id: &QString);

        /// A click in the command list: a category button or a command row.
        #[qinvokable]
        fn pick(self: Pin<&mut Commander>, what: &QString, index: i32);

        /// A click on a row of a panel (side 0 or 1, row from the first shown).
        #[qinvokable]
        fn click_row(self: Pin<&mut Commander>, side: i32, row: i32);

        /// The wheel over a panel.
        #[qinvokable]
        fn scroll_rows(self: Pin<&mut Commander>, side: i32, lines: i32);

        /// A row dragged from one panel onto the other.
        #[qinvokable]
        fn drag_rows(self: Pin<&mut Commander>, from: i32, row: i32, to: i32);

        /// Files dropped on a panel from another program (`file:` URLs, one
        /// per line).
        #[qinvokable]
        fn drop_files(self: Pin<&mut Commander>, side: i32, urls: &QString);

        /// The window's size when it is closed, kept for the next start.
        #[qinvokable]
        fn remember_window(self: Pin<&mut Commander>, width: i32, height: i32);

        /// Ends the application.
        #[qinvokable]
        fn quit(self: Pin<&mut Commander>);

        /// How many rows fit in a panel and in the viewer.
        #[qinvokable]
        fn resize(self: Pin<&mut Commander>, rows: i32, viewer_rows: i32, columns: i32);
    }

    impl cxx_qt::Threading for Commander {}
}

unsafe extern "C" {
    fn nmc_set_window_icon(data: *const u8, length: u64);
}

/// Adds a size (a PNG) to the icon of every window of the application.
pub fn add_window_icon(png: &[u8]) {
    // SAFETY: the pointer and length describe `png`, which outlives the call;
    // Qt copies what it needs.
    unsafe { nmc_set_window_icon(png.as_ptr(), png.len() as u64) }
}
