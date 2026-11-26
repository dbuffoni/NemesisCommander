//! Compiles the Rust/Qt bridge and bundles the QML files into the binary.

use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    // macdeployqt rewrites the Qt paths of the program so that it finds the
    // copy inside the app bundle: the new ones are longer and need room.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-headerpad_max_install_names");
    }
    let module = QmlModule::new("NemesisCommander").qml_files([
        "qml/Main.qml",
        "qml/PanelView.qml",
        "qml/ViewerView.qml",
        "qml/DialogView.qml",
        "qml/Box.qml",
    ]);
    CxxQtBuilder::new_qml_module(module).files(["src/ffi.rs"]).cpp_file("src/icon.cpp").build();
}
