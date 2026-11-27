//! Compiles the Rust/Qt bridge and bundles the QML files into the binary.

use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    // macdeployqt rewrites the Qt paths of the program so that it finds the
    // copy inside the app bundle: the new ones are longer and need room.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-headerpad_max_install_names");
    }
    embed_windows_icon();
    let module = QmlModule::new("Pairnox").qml_files([
        "qml/Main.qml",
        "qml/PanelView.qml",
        "qml/ViewerView.qml",
        "qml/DialogView.qml",
        "qml/Box.qml",
    ]);
    CxxQtBuilder::new_qml_module(module).files(["src/ffi.rs"]).cpp_file("src/icon.cpp").build();
}

/// The program's own icon on Windows: the `.ico` of the brand folder, as the
/// icon resource of `pnxr-gui.exe`.
#[cfg(windows)]
fn embed_windows_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../assets/branding/pairnox.ico");
        println!("cargo:rerun-if-changed={}", icon.display());
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon(icon.to_str().expect("the icon path is text"));
        resource.set("ProductName", "Pairnox");
        resource.set("FileDescription", "Pairnox");
        resource.compile().expect("the Windows icon resource compiles");
    }
}

/// Nothing to embed when the window is not built on Windows.
#[cfg(not(windows))]
fn embed_windows_icon() {}
