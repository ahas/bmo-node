//! The Windows sensor setup: done only where both the library and the driver answered, a machine with no
//! Windows left alone, and a Windows machine reached in its own PowerShell handed the setup script itself
//! over the hardware monitor's road.

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use node_server::cli::{WindowsSetup, read_setup, windows_setup_script, windows_shell_line};
use policy::windows_sensors::WINDOWS_SENSORS;
use store::Store;
use store::nodes::SshLogin;

#[test]
fn the_setup_is_done_only_where_both_the_library_and_the_driver_answered() {
  let release = &WINDOWS_SENSORS.release;
  let placed = format!("library {release} already in place\r\ndriver PawnIO installed\r\n");

  assert_eq!(
    read_setup(&placed, Ok(())),
    WindowsSetup::Done(vec![
      format!("library {release} already in place"),
      "driver PawnIO installed".into()
    ])
  );
  assert_eq!(read_setup("@@not-windows\n", Ok(())), WindowsSetup::NotWindows);
  assert!(matches!(
    read_setup("library v1 already in place\nthe PawnIO driver did not install\n", Ok(())),
    WindowsSetup::Failed { detail, .. } if detail == "the setup did not report both its library and its driver"
  ));
  assert!(matches!(
    read_setup("", Err("the script ended with 1".into())),
    WindowsSetup::Failed { detail, .. } if detail == "the script ended with 1"
  ));
  assert!(windows_shell_line("x").starts_with("if grep -qi microsoft /proc/version"));
}

/// An OpenSSH stand-in on the search path: it writes down its arguments and answers as a Windows that holds
/// both the library and the driver.
fn ssh_stand_in(folder: &Path) -> String {
  let ssh = folder.join("ssh");

  std::fs::write(
    &ssh,
    format!(
      "#!/bin/sh\nfor word in \"$@\"; do printf '%s\\n' \"$word\"; done > '{}'\nprintf 'library v0 already in place\\r\\ndriver PawnIO already installed\\r\\n'\n",
      folder.join("handed").display()
    ),
  )
  .unwrap();
  std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();

  format!("{}:{}", folder.display(), std::env::var("PATH").unwrap_or_default())
}

/// The script a PowerShell command carries, unpacked.
fn carried(command: &str) -> String {
  let encoded = command.strip_prefix("powershell -NoProfile -NonInteractive -EncodedCommand ").unwrap();
  let units: Vec<u16> = STANDARD
    .decode(encoded)
    .unwrap()
    .chunks(2)
    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
    .collect();
  let unpack = String::from_utf16(&units).unwrap();
  let packed = unpack.split('\'').nth(1).unwrap();
  let mut script = String::new();

  flate2::read::GzDecoder::new(STANDARD.decode(packed).unwrap().as_slice())
    .read_to_string(&mut script)
    .unwrap();
  script
}

fn run(dir: &Path, path: &str, words: &[&str]) -> (i32, String, String) {
  let done = Command::new(crate::binary::built("node-server", "bmo-node"))
    .arg("--config")
    .arg(dir.join("node.toml"))
    .args(words)
    .env("PATH", path)
    .env("HOME", dir)
    .stdin(Stdio::null())
    .output()
    .unwrap();

  (
    done.status.code().unwrap_or(-1),
    String::from_utf8_lossy(&done.stdout).into_owned(),
    String::from_utf8_lossy(&done.stderr).into_owned(),
  )
}

#[test]
fn a_windows_machine_reached_in_its_own_powershell_is_handed_the_setup_script_itself() {
  let dir = tempfile::tempdir().unwrap();
  let data = dir.path().join("data");
  let path = ssh_stand_in(dir.path());

  std::fs::write(
    dir.path().join("node.toml"),
    format!("listen = \"127.0.0.1:0\"\ntoken = \"t\"\ndata_root = \"{}\"\n", data.display()),
  )
  .unwrap();
  Store::open_in(&data)
    .unwrap()
    .add_node(
      "win",
      None,
      None,
      SshLogin {
        shell: "powershell".into(),
        ..SshLogin::new("192.0.2.9", "me")
      },
    )
    .unwrap();

  let (code, out, err) = run(dir.path(), &path, &["hardware-setup", "win"]);
  let handed = std::fs::read_to_string(dir.path().join("handed")).unwrap();

  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "win: library v0 already in place; driver PawnIO already installed\n");
  assert!(handed.lines().any(|word| word == "me@192.0.2.9"));
  assert_eq!(carried(handed.lines().last().unwrap()), windows_setup_script());

  // A node with no row of its own is refused before anything runs.
  let (code, _, err) = run(dir.path(), &path, &["hardware-setup", "nowhere"]);

  assert_eq!((code, err.as_str()), (1, "no such node: nowhere\n"));

  // The node verbs run through the command over the same store.
  let (code, out, _) = run(dir.path(), &path, &["node"]);

  assert_eq!(code, 0);
  assert!(out.contains("win [ssh] win — me@192.0.2.9 (PowerShell), key (ssh config)"), "{out}");

  let (code, _, err) = run(dir.path(), &path, &["node", "--remove", "nowhere"]);

  assert_eq!((code, err.as_str()), (1, "no node named nowhere\n"));
}
