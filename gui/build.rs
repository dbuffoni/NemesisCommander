//! Compiles the Rust/Qt bridge and bundles the QML files into the binary.

use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let module = QmlModule::new("SuperCommander").qml_files([
        "qml/Main.qml",
        "qml/PanelView.qml",
        "qml/ViewerView.qml",
        "qml/DialogView.qml",
        "qml/Box.qml",
    ]);
    CxxQtBuilder::new_qml_module(module).files(["src/ffi.rs"]).build();
}
