//! Helpers for starting helper programs without a console window.
//!
//! The Windows GUI has no console of its own, so every console program it
//! starts (`git`, `ssh`, `powershell`, the `pnxr` command line) would open a
//! console window for a moment. Output is read through pipes anyway, so the
//! window is never wanted.

use std::process::Command;

/// `CREATE_NO_WINDOW` of the Windows `CreateProcess` call.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Keeps the program from opening a console window on Windows; it changes
/// nothing on other systems. Use it on every program started to read its
/// output, never for one the user must see in a terminal.
pub fn hide_console(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}
