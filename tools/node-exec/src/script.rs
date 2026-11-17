//! The handing script a command is wrapped in where a secret is handed as a file (docs/tools.md, 「Secrets handed
//! by name」): it writes each file out of the variable carrying it into a folder of the call's own in the user's
//! temporary folder, runs the command, and removes the folder however the command ends; and the command the tool
//! sends after the command's end, removing the folder where a kill skipped the script's own removal. A script
//! names variables alone and holds no value. Every command of the `wsl` road is sent encoded, whatever it holds
//! (docs/tools.md, 「A call」).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bmo_node_client::nodes::LoginShell;
use bmo_node_client::runs::RunShell;

use crate::secrets::Carried;

/// The prefix of a call's folder, which a later handing script sweeps once a day old.
const FOLDER_HEAD: &str = "bmo-secret-";

/// The road a command takes to the shell that runs it, as far as what the tool sends differs by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
  /// bash, a Linux host's shell, taking the command as written.
  Bash,
  /// bash in WSL on a Windows host, entered through `wsl.exe`, whose distribution's default shell expands the
  /// command line once before bash takes it.
  Wsl,
  /// Windows PowerShell.
  PowerShell,
}

impl Script {
  /// The road of a command in `shell` on a host whose own shell is `host`; none where the host does not run
  /// that shell, since the agent would run the command in its host's own instead.
  pub fn of(shell: RunShell, host: LoginShell) -> Option<Self> {
    match (host, shell) {
      (LoginShell::Posix, RunShell::Linux) => Some(Self::Bash),
      (LoginShell::Powershell, RunShell::Wsl) => Some(Self::Wsl),
      (LoginShell::Powershell, RunShell::Powershell) => Some(Self::PowerShell),
      _ => None,
    }
  }

  /// The shells a host whose own shell is `host` runs, as a refusal names them.
  pub fn runs(host: LoginShell) -> &'static str {
    match host {
      LoginShell::Posix => "linux",
      LoginShell::Powershell => "powershell and wsl",
    }
  }

  /// `command` as it is sent on this road. A `wsl` command goes base64-encoded inside a line holding no character
  /// a shell expands, decoded and handed to bash on its standard input inside one group, which bash reads whole
  /// before it runs any of it, so the command reaches bash as written and reads nothing on its own standard input.
  pub fn sent(self, command: String) -> String {
    match self {
      Self::Wsl => format!("echo {} | base64 -d | bash", STANDARD.encode(format!("{{\n{command}\n}}\n"))),
      Self::Bash | Self::PowerShell => command,
    }
  }

  /// `command` wrapped so it finds `files` written in the folder `id` names, removed at its end.
  pub fn wrap(self, id: &str, files: &[Carried], command: &str) -> String {
    match self {
      Self::Bash | Self::Wsl => bash_wrap(id, files, command),
      Self::PowerShell => powershell_wrap(id, files, command),
    }
  }

  /// The command removing the folder `id` names, ending with 0 where it is gone.
  pub fn removal(self, id: &str) -> String {
    match self {
      Self::Bash | Self::Wsl => format!("rm -rf \"${{TMPDIR:-/tmp}}/{FOLDER_HEAD}{id}\""),
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

/// The folder is made in PowerShell's temporary path, the user's own `%TEMP%` on Windows, and on Windows given an
/// access list of its own holding the user alone and inheriting nothing, before any file is written in it, since
/// a user's temporary folder may grant others too; PowerShell on another system, which has no such list, skips
/// it. The command runs in a `try` whose `finally` removes the folder. The process ends with 1 where the
/// command's last statement failed, as PowerShell ends a command it is handed.
fn powershell_wrap(id: &str, files: &[Carried], command: &str) -> String {
  let mut script = format!(
    "$__bmoTemp = [System.IO.Path]::GetTempPath()\n\
     Get-ChildItem -LiteralPath $__bmoTemp -Directory -Filter '{FOLDER_HEAD}*' -ErrorAction SilentlyContinue | \
     Where-Object {{ $_.LastWriteTime -lt (Get-Date).AddDays(-1) }} | \
     Remove-Item -Recurse -Force -ErrorAction SilentlyContinue\n\
     $__bmoDir = Join-Path $__bmoTemp '{FOLDER_HEAD}{id}'\n\
     $null = New-Item -ItemType Directory -Path $__bmoDir -ErrorAction Stop\n\
     $__bmoOk = $false\n\
     try {{\n\
     if ($IsWindows -ne $false) {{\n\
     $__bmoAcl = New-Object System.Security.AccessControl.DirectorySecurity\n\
     $__bmoAcl.SetAccessRuleProtection($true, $false)\n\
     $__bmoAcl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule(\
     [System.Security.Principal.WindowsIdentity]::GetCurrent().User, 'FullControl', \
     'ContainerInherit, ObjectInherit', 'None', 'Allow')))\n\
     Set-Acl -LiteralPath $__bmoDir -AclObject $__bmoAcl -ErrorAction Stop\n\
     }}\n"
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
