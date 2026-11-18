//! Writes the sample files of `nmc_preview::samples` to a folder:
//! `cargo run -p nmc-preview --example samples -- DIR`.

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: samples DIR");
        std::process::exit(2);
    };
    std::fs::create_dir_all(&dir).expect("creating the folder");
    for (name, data) in nmc_preview::samples::all() {
        let path = std::path::Path::new(&dir).join(name);
        std::fs::write(&path, data).expect("writing a sample");
        println!("{}", path.display());
    }
}
