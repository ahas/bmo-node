//! The handing script a command is wrapped in where a secret is handed as a file (docs/tools.md, 「Secrets handed
//! by name」): it writes each file out of the variable carrying it into a folder of the call's own in the user's
//! temporary folder, runs the command, and removes the folder however the command ends; and the command the tool
//! sends after the command's end, removing the folder where a kill skipped the script's own removal. A script
//! names variables alone and holds no value.

use bmo_node_client::nodes::LoginShell;
use bmo_node_client::runs::RunShell;

use crate::secrets::Carried;

/// The prefix of a call's folder, which a later handing script sweeps once a day old.
const FOLDER_HEAD: &str = "bmo-secret-";

/// The language the agent runs a command in, as far as a handing script differs by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
  /// bash, the Linux host's shell and the one `wsl` enters.
  Bash,
  /// Windows PowerShell.
  PowerShell,
}

impl Script {
  /// The script's language for a command in `shell` on a host whose own shell is `host`: the agent runs every
  /// command of a Linux host in bash, whatever shell it names.
  pub fn of(shell: RunShell, host: Option<LoginShell>) -> Self {
    match (shell, host) {
      (RunShell::Powershell, Some(LoginShell::Posix)) => Self::Bash,
      (RunShell::Powershell, _) => Self::PowerShell,
      (RunShell::Wsl | RunShell::Linux, _) => Self::Bash,
    }
  }

  /// `command` wrapped so it finds `files` written in the folder `id` names, removed at its end.
  pub fn wrap(self, id: &str, files: &[Carried], command: &str) -> String {
    match self {
      Self::Bash => bash_wrap(id, files, command),
      Self::PowerShell => powershell_wrap(id, files, command),
    }
  }

  /// The command removing the folder `id` names, ending with 0 where it is gone.
  pub fn removal(self, id: &str) -> String {
    match self {
      Self::Bash => format!("rm -rf \"${{TMPDIR:-/tmp}}/{FOLDER_HEAD}{id}\""),
      Self::PowerShell => format!(
        "$__bmoDir = Join-Path ([System.IO.Path]::GetTempPath()) '{FOLDER_HEAD}{id}'\n\
         if (Test-Path -LiteralPath $__bmoDir) {{ Remove-Item -LiteralPath $__bmoDir -Recurse -Force -ErrorAction Stop }}"
      ),
    }
  }
}

/// The folder is made under a umask of 077 and each file written under one, so both are the user's alone; the
/// command runs in a subshell, so a trap it sets replaces none of the script's, and the script exits with its
/// status by an `exit` of its own after it: bash runs a subshell that ends a script in its own process, which
/// would drop the script's traps. A hangup, an interrupt or a termination ends the script through its exit trap.
fn bash_wrap(id: &str, files: &[Carried], command: &str) -> String {
  let mut script = format!(
    "__bmo_tmp=\"${{TMPDIR:-/tmp}}\"\n\
     find \"$__bmo_tmp\" -maxdepth 1 -name '{FOLDER_HEAD}*' -user \"$(id -u)\" -mmin +1440 -exec rm -rf {{}} + 2>/dev/null\n\
     __bmo_dir=\"$__bmo_tmp/{FOLDER_HEAD}{id}\"\n\
     (umask 077 && mkdir \"$__bmo_dir\") || exit 1\n\
     trap 'rm -rf \"$__bmo_dir\"' EXIT\n\
     trap 'exit 129' HUP\n\
     trap 'exit 130' INT\n\
     trap 'exit 143' TERM\n"
  );

  for Carried {
    carrier,
    path,
    file,
  } in files
  {
    script.push_str(&format!(
      "(umask 077 && printf '%s' \"${carrier}\" | base64 -d > \"$__bmo_dir/{file}\") || exit 1\n\
       unset {carrier}\n\
       export {path}=\"$__bmo_dir/{file}\"\n"
    ));
  }

  script.push_str(&format!("(\n{command}\n)\n__bmo_status=$?\nexit \"$__bmo_status\"\n"));
  script
}

/// The folder is made in PowerShell's temporary path, the user's own `%TEMP%` on Windows, whose access list
/// holds the user alone; the command runs in a `try` whose `finally` removes it. The process ends with 1 where
/// the command's last statement failed, as PowerShell ends a command it is handed.
fn powershell_wrap(id: &str, files: &[Carried], command: &str) -> String {
  let mut script = format!(
    "$__bmoTemp = [System.IO.Path]::GetTempPath()\n\
     Get-ChildItem -LiteralPath $__bmoTemp -Directory -Filter '{FOLDER_HEAD}*' -ErrorAction SilentlyContinue | \
     Where-Object {{ $_.LastWriteTime -lt (Get-Date).AddDays(-1) }} | \
     Remove-Item -Recurse -Force -ErrorAction SilentlyContinue\n\
     $__bmoDir = Join-Path $__bmoTemp '{FOLDER_HEAD}{id}'\n\
     $null = New-Item -ItemType Directory -Path $__bmoDir -ErrorAction Stop\n\
     $__bmoOk = $false\n\
     try {{\n"
  );

  for Carried {
    carrier,
    path,
    file,
  } in files
  {
    script.push_str(&format!(
      "[System.IO.File]::WriteAllBytes((Join-Path $__bmoDir '{file}'), \
       [System.Convert]::FromBase64String($env:{carrier}))\n\
       Remove-Item -LiteralPath Env:{carrier}\n\
       $env:{path} = Join-Path $__bmoDir '{file}'\n"
    ));
  }

  script.push_str(&format!(
    "{command}\n\
     $__bmoOk = $?\n\
     }} finally {{\n\
     Remove-Item -LiteralPath $__bmoDir -Recurse -Force -ErrorAction SilentlyContinue\n\
     }}\n\
     if (-not $__bmoOk) {{ exit 1 }}\n"
  ));
  script
}

/// A folder's id, random to the call.
pub fn folder_id() -> Result<String, String> {
  let mut bytes = [0u8; 16];

  getrandom::fill(&mut bytes)
    .map_err(|why| format!("no random id could be had for the secrets' folder: {why}"))?;

  Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
