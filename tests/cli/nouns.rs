//! The nouns the command declares for `bmo`, and the configuration a noun's words may carry after them.

use std::process::{Command, Stdio};

fn run(words: &[&str]) -> (i32, String, String) {
  let done = Command::new(crate::binary::built("node-server", "bmo-node"))
    .args(words)
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
fn the_command_declares_the_node_list_and_the_sensor_setup_for_bmo() {
  let (code, stdout, stderr) = run(&["nouns"]);

  assert_eq!(code, 0, "{stderr}");
  assert_eq!(stdout, "node\thome\nhardware-setup\thome\n");
}

#[test]
fn a_nouns_words_take_the_configuration_after_the_noun() {
  let dir = tempfile::tempdir().unwrap();
  let config = dir.path().join("node.toml");

  std::fs::write(
    &config,
    format!(
      "listen = \"127.0.0.1:0\"\ntoken = \"t\"\ndata_root = \"{}\"\n",
      dir.path().join("data").display()
    ),
  )
  .unwrap();

  let (code, stdout, stderr) = run(&["node", "--config", config.to_str().unwrap()]);

  assert_eq!(code, 0, "{stderr}");
  assert!(stdout.starts_with("(no nodes)\n"), "{stdout}");
}
