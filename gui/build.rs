//! Compiles the Rust/Qt bridge and bundles the QML files into the binary.

use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let module = QmlModule::new("NemesisCommander").qml_files([
        "qml/Main.qml",
        "qml/PanelView.qml",
        "qml/ViewerView.qml",
        "qml/DialogView.qml",
        "qml/Box.qml",
    ]);
    CxxQtBuilder::new_qml_module(module).files(["src/ffi.rs"]).cpp_file("src/icon.cpp").build();
}
