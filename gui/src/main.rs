//! The `pnxr-gui` program: reads the command line and opens the window (see
//! the library for how the window works).

const USAGE: &str = "usage: pnxr-gui [LEFT] [RIGHT]\n\n\
    Opens Pairnox's window with the two locations (folders or URLs\n\
    such as sftp://host/path); the current folder when left out.\n\n\
    For tests: --self-test [--keys=\"F1 Esc\"] [--screenshot=FILE.png] presses\n\
    the keys once the listings are loaded, prints the screen as JSON, saves\n\
    a picture of the window and quits.";

fn main() {
    let mut startup = pnxr_gui::Startup::default();
    let mut places = Vec::new();
    let (mut test, mut keys, mut shot) = (None, String::new(), None);
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            "-V" | "--version" => {
                println!("pnxr-gui {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--self-test" => test = Some(pnxr_gui::SelfTest::default()),
            _ if argument.starts_with("--keys=") => keys = argument[7..].to_owned(),
            _ if argument.starts_with("--screenshot=") => shot = Some(argument[13..].to_owned()),
            _ if argument.starts_with("--size=") => match parse_size(&argument[7..]) {
                Some(size) => startup.size = Some(size),
                None => {
                    eprintln!("pnxr-gui: --size takes WIDTHxHEIGHT, such as 1280x800");
                    std::process::exit(2);
                }
            },
            _ => places.push(argument),
        }
    }
    if let Some(test) = &mut test {
        test.keys = keys.split_whitespace().map(str::to_owned).collect();
        test.screenshot = shot;
    }
    startup.self_test = test;
    if places.len() > 2 {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let mut places = places.into_iter();
    startup.left = places.next();
    startup.right = places.next();
    // A launcher written by `pnxr desktop install` gets the current icon.
    if startup.self_test.is_none() {
        pnxr_app::desktop::refresh();
    }
    if !pnxr_gui::run(startup) {
        std::process::exit(1);
    }
}

/// `1280x800` as (1280, 800); at least 320×200.
fn parse_size(text: &str) -> Option<(i32, i32)> {
    let (width, height) = text.split_once('x')?;
    let (width, height) = (width.parse().ok()?, height.parse().ok()?);
    (width >= 320 && height >= 200).then_some((width, height))
}
