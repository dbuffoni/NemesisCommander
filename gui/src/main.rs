//! The `sc-gui` program: reads the command line and opens the window (see
//! the library for how the window works).

const USAGE: &str = "usage: sc-gui [LEFT] [RIGHT]\n\n\
    Opens SuperCommander's window with the two locations (folders or URLs\n\
    such as sftp://host/path); the current folder when left out.\n\n\
    For tests: --self-test [--keys=\"F1 Esc\"] [--screenshot=FILE.png] presses\n\
    the keys once the listings are loaded, prints the screen as JSON, saves\n\
    a picture of the window and quits.";

fn main() {
    let mut startup = sc_gui::Startup::default();
    let mut places = Vec::new();
    let (mut test, mut keys, mut shot) = (None, String::new(), None);
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            "-V" | "--version" => {
                println!("sc-gui {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--self-test" => test = Some(sc_gui::SelfTest::default()),
            _ if argument.starts_with("--keys=") => keys = argument[7..].to_owned(),
            _ if argument.starts_with("--screenshot=") => shot = Some(argument[13..].to_owned()),
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
    if !sc_gui::run(startup) {
        std::process::exit(1);
    }
}
