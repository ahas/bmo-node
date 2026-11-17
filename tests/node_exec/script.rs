//! The handing scripts run in the shells themselves (docs/tools.md, 「Secrets handed by name」): bash as the agent
//! runs a Linux command; bash behind the shell `wsl.exe` hands a WSL command line to first, which expands it once;
//! and PowerShell as it runs a Windows one, here `pwsh`; each with its temporary folder pointed into a folder of
//! the test's own. A file's bytes arrive whole, the carrying variable is gone before the command, the folder is
//! gone after it however the command ends, and the command's status stands.

use std::path::Path;
use std::process::{Command, Output};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use node_exec_tool::script::Script;
use node_exec_tool::secrets::Carried;

const ID: &str = "0123456789abcdef0123456789abcdef";

/// Bytes no shell's quoting or newline handling may change.
const BYTES: &[u8] = b"line one\nquote ' and \" and $HOME\n\0tail";

fn carried() -> Vec<Carried> {
  vec![Carried {
    carrier: "BMO_SECRET_BYTES_0".into(),
    path: "BMO_SECRET_TEST_KEY_FILE".into(),
    file: "0".into(),
  }]
}

/// `text` sent on `script`'s road and run as the agent runs it, with `tmp` as the temporary folder and the
/// file's bytes carried. A WSL command goes through the shell `wsl.exe` hands its command line to: the line the
/// agent's `bash -lc` and its argument make, the argument in double quotes, expanded once before bash takes it.
fn run(script: Script, text: &str, tmp: &Path) -> Output {
  let sent = script.sent(text.to_owned());
  let mut command = match script {
    Script::Bash => {
      let mut bash = Command::new("bash");

      bash.args(["-lc", &sent]);
      bash
    }
    Script::Wsl => {
      let mut outer = Command::new("bash");

      assert!(!sent.contains(['"', '\\', '$', '`', '\'']), "{sent}");
      outer.args(["-c", &format!("bash -lc \"{sent}\"")]);
      outer
    }
    Script::PowerShell => {
      let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
      let mut pwsh = Command::new("pwsh");

      assert_eq!(sent, text);
      pwsh.args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &STANDARD.encode(utf16)]);
      pwsh
    }
  };

  command
    .stdin(std::process::Stdio::null())
    .env("TMPDIR", tmp)
    .env("BMO_SECRET_BYTES_0", STANDARD.encode(BYTES))
    .output()
    .unwrap()
}

fn said(output: &Output) -> String {
  format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

fn folder(tmp: &Path) -> std::path::PathBuf {
  tmp.join(format!("bmo-secret-{ID}"))
}

/// The command the file is checked by: the file copied out beside the folder, with its mode, and whether the
/// carrying variable is still set.
fn checked(script: Script, tmp: &Path) -> String {
  let copy = tmp.join("copy");

  match script {
    Script::Bash | Script::Wsl => format!(
      "cp \"$BMO_SECRET_TEST_KEY_FILE\" {copy}\n\
       stat -c '%a' \"$BMO_SECRET_TEST_KEY_FILE\" \"$(dirname \"$BMO_SECRET_TEST_KEY_FILE\")\" > {copy}.mode\n\
       echo \"carrier:${{BMO_SECRET_BYTES_0-gone}}\"",
      copy = copy.display()
    ),
    Script::PowerShell => format!(
      "Copy-Item -LiteralPath $env:BMO_SECRET_TEST_KEY_FILE -Destination '{copy}'\n\
       Write-Output \"carrier:$(if (Test-Path Env:BMO_SECRET_BYTES_0) {{ 'set' }} else {{ 'gone' }})\"",
      copy = copy.display()
    ),
  }
}

/// The temporary folder the file arrived in, for what a shell's own test reads beside it.
fn the_file_arrives_whole_and_the_folder_goes(script: Script) -> tempfile::TempDir {
  let tmp = tempfile::tempdir().unwrap();
  let output = run(script, &script.wrap(ID, &carried(), &checked(script, tmp.path())), tmp.path());

  assert_eq!(output.status.code(), Some(0), "{}", said(&output));
  assert!(String::from_utf8_lossy(&output.stdout).contains("carrier:gone"), "{}", said(&output));
  assert_eq!(std::fs::read(tmp.path().join("copy")).unwrap(), BYTES);
  assert!(!folder(tmp.path()).exists());
  tmp
}

fn the_command_s_status_stands_and_the_folder_goes_however_it_ends(script: Script, ends: &[(&str, i32)]) {
  for (command, code) in ends {
    let tmp = tempfile::tempdir().unwrap();
    let output = run(script, &script.wrap(ID, &carried(), command), tmp.path());

    assert_eq!(output.status.code(), Some(*code), "{command}: {}", said(&output));
    assert!(!folder(tmp.path()).exists(), "{command}");
  }
}

fn the_removal_takes_a_folder_left_and_ends_with_0_where_none_is(script: Script) {
  let tmp = tempfile::tempdir().unwrap();

  std::fs::create_dir(folder(tmp.path())).unwrap();
  std::fs::write(folder(tmp.path()).join("0"), BYTES).unwrap();

  for _ in 0..2 {
    let output = run(script, &script.removal(ID), tmp.path());

    assert_eq!(output.status.code(), Some(0), "{}", said(&output));
    assert!(!folder(tmp.path()).exists());
  }
}

#[test]
fn bash_writes_the_file_the_user_s_alone_and_removes_it() {
  let tmp = the_file_arrives_whole_and_the_folder_goes(Script::Bash);
  let modes = std::fs::read_to_string(tmp.path().join("copy.mode")).unwrap();

  assert_eq!(modes.lines().collect::<Vec<_>>(), ["600", "700"]);
}

const BASH_ENDS: [(&str, i32); 4] = [
  ("exit 3", 3),
  ("false", 1),
  ("trap 'echo bye' EXIT; trap - TERM; true", 0),
  ("kill -TERM $$", 143),
];

#[test]
fn bash_keeps_the_command_s_status_and_its_own_trap_replaces_none_of_the_script_s() {
  the_command_s_status_stands_and_the_folder_goes_however_it_ends(Script::Bash, &BASH_ENDS);
}

#[test]
fn bash_sweeps_a_folder_a_day_old_and_leaves_a_new_one() {
  let tmp = tempfile::tempdir().unwrap();
  let old = tmp.path().join("bmo-secret-old");
  let new = tmp.path().join("bmo-secret-new");

  std::fs::create_dir(&old).unwrap();
  std::fs::create_dir(&new).unwrap();

  let day_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600);

  std::fs::File::open(&old).unwrap().set_modified(day_ago).unwrap();

  let output = run(Script::Bash, &Script::Bash.wrap(ID, &carried(), "true"), tmp.path());

  assert_eq!(output.status.code(), Some(0), "{}", said(&output));
  assert!(!old.exists() && new.exists());
}

#[test]
fn bash_removal_takes_a_folder_left() {
  the_removal_takes_a_folder_left_and_ends_with_0_where_none_is(Script::Bash);
}

#[test]
fn wsl_hands_the_file_through_the_outer_shell_the_user_s_alone_and_removes_it() {
  let tmp = the_file_arrives_whole_and_the_folder_goes(Script::Wsl);
  let modes = std::fs::read_to_string(tmp.path().join("copy.mode")).unwrap();

  assert_eq!(modes.lines().collect::<Vec<_>>(), ["600", "700"]);
}

#[test]
fn wsl_keeps_the_command_s_status_and_its_traps_through_the_outer_shell() {
  the_command_s_status_stands_and_the_folder_goes_however_it_ends(Script::Wsl, &BASH_ENDS);
}

#[test]
fn wsl_hands_a_command_to_bash_as_written_and_its_standard_input_holds_nothing() {
  let tmp = tempfile::tempdir().unwrap();
  let output = run(
    Script::Wsl,
    "x='in \"bash\"'; echo \"[$x] [`echo $((1 + 1))`]\"\ncat\necho after",
    tmp.path(),
  );

  assert_eq!(output.status.code(), Some(0), "{}", said(&output));
  assert_eq!(String::from_utf8_lossy(&output.stdout), "[in \"bash\"] [2]\nafter\n");
}

#[test]
fn wsl_removal_takes_a_folder_left() {
  the_removal_takes_a_folder_left_and_ends_with_0_where_none_is(Script::Wsl);
}

#[test]
fn powershell_writes_the_file_and_removes_it() {
  the_file_arrives_whole_and_the_folder_goes(Script::PowerShell);
}

#[test]
fn powershell_keeps_the_command_s_status_however_it_ends() {
  the_command_s_status_stands_and_the_folder_goes_however_it_ends(
    Script::PowerShell,
    &[
      ("exit 3", 3),
      ("Write-Output fine", 0),
      ("Get-Item -LiteralPath '/nowhere/at/all'", 1),
      ("throw 'stop'", 1),
    ],
  );
}

#[test]
fn powershell_removal_takes_a_folder_left() {
  the_removal_takes_a_folder_left_and_ends_with_0_where_none_is(Script::PowerShell);
}
