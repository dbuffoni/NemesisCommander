//! `nmc shell-init`: the shell function that leaves the shell in the folder
//! the terminal interface ended in.

use std::process::ExitCode;

use crate::cli::Shell;
use crate::commands::CmdResult;

/// The code to put in the shell's startup file: a function `nmc` that runs
/// the program with `--cwd-file` and then changes to the folder it wrote.
/// Commands (`nmc ls`, `nmc cp`...) are passed through unchanged: the
/// folder is only read when the file has something in it.
pub fn snippet(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash | Shell::Zsh => {
            r#"nmc() {
  if [ "$#" -gt 0 ] && case "$1" in -*|ls|cp|mv|rm|mkdir|cat|hash|find|pack|unpack|rename|jobs|config|keys|agent|serve|tls|run|plugin|desktop|test|ai|pick|shell-init|help) true;; *) false;; esac; then
    command nmc "$@"; return
  fi
  local cwd_file; cwd_file="$(mktemp "${TMPDIR:-/tmp}/nmc-cwd.XXXXXX")" || return
  command nmc --cwd-file "$cwd_file" "$@"
  local code=$? dir
  dir="$(cat -- "$cwd_file" 2>/dev/null)"
  rm -f -- "$cwd_file"
  if [ -n "$dir" ] && [ "$dir" != "$PWD" ] && [ -d "$dir" ]; then cd -- "$dir"; fi
  return $code
}
"#
        }
        Shell::Fish => {
            r#"function nmc
    if test (count $argv) -gt 0
        switch $argv[1]
            case '-*' ls cp mv rm mkdir cat hash find pack unpack rename jobs config keys agent serve tls run plugin desktop test ai pick shell-init help
                command nmc $argv
                return
        end
    end
    set -l cwd_file (mktemp)
    command nmc --cwd-file $cwd_file $argv
    set -l code $status
    set -l dir (cat $cwd_file 2>/dev/null)
    rm -f $cwd_file
    if test -n "$dir"; and test "$dir" != "$PWD"; and test -d "$dir"
        cd $dir
    end
    return $code
end
"#
        }
        Shell::Pwsh => {
            r#"function nmc {
    $passThrough = @('ls','cp','mv','rm','mkdir','cat','hash','find','pack','unpack','rename','jobs','config','keys','agent','serve','tls','run','plugin','desktop','test','ai','pick','shell-init','help')
    $exe = (Get-Command nmc -CommandType Application | Select-Object -First 1).Source
    if ($args.Count -gt 0 -and ($args[0] -like '-*' -or $passThrough -contains $args[0])) {
        & $exe @args
        return
    }
    $cwdFile = New-TemporaryFile
    & $exe --cwd-file $cwdFile.FullName @args
    $dir = Get-Content -Raw -ErrorAction SilentlyContinue $cwdFile.FullName
    Remove-Item -Force $cwdFile.FullName
    if ($dir) { $dir = $dir.Trim() }
    if ($dir -and (Test-Path -PathType Container $dir) -and ($dir -ne (Get-Location).Path)) {
        Set-Location -LiteralPath $dir
    }
}
"#
        }
    }
}

/// Where the snippet goes, for the help of the command.
pub fn shell_init(shell: Shell) -> CmdResult {
    crate::out!("{}", snippet(shell).trim_end());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shell_passes_the_file_and_changes_folder() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Pwsh] {
            let text = snippet(shell);
            assert!(text.contains("--cwd-file"), "{shell:?}");
            assert!(text.contains("nmc"), "{shell:?}");
            assert!(text.contains("cd") || text.contains("Set-Location"), "{shell:?}");
        }
    }

    #[test]
    fn the_bash_function_changes_the_folder_for_real() {
        // A fake `nmc` that writes /tmp as the folder it ended in.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("nmc");
        std::fs::write(
            &fake,
            "#!/bin/sh\nif [ \"$1\" = --cwd-file ]; then printf %s /tmp > \"$2\"; fi\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let script = format!("{}\ncd /; nmc; pwd; nmc ls; pwd", snippet(Shell::Bash));
        let out = std::process::Command::new("bash")
            .args(["-c", &script])
            .env("PATH", format!("{}:{}", dir.path().display(), std::env::var("PATH").unwrap()))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            ["/tmp", "/tmp"],
            "{text}{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
