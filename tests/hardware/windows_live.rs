//! The Windows live scripts run in `pwsh` as their roads run them, their queries stood in for (docs/hardware.md,
//! 「The Windows side」): the first block comes before anything slow, the first reading carries the answers that
//! came, and a system query that hangs costs only a fresh answer, never the block of every second.

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hardware::windows::{encoded_command, packed_script};
use regex::Regex;

/// How many readings after the system query hangs the run must still print.
const AFTER_THE_HANG: usize = 6;

/// The script with its queries stood in for, asked every three readings, paced at a tenth of a second: the
/// system query answers once and hangs when asked again, leaving a file behind that says it was.
fn stood(script: &str, dir: &Path) -> String {
  let answered = dir.join("answered").to_string_lossy().into_owned();
  let hanging = dir.join("hanging").to_string_lossy().into_owned();
  let system = format!(
    "$system = NewAsking @'\n\
     if (Test-Path '{answered}') {{ New-Item -ItemType File '{hanging}' | Out-Null; Start-Sleep -Seconds 600 }}\n\
     New-Item -ItemType File '{answered}' | Out-Null\n\
     [pscustomobject]@{{ lines = @(\"os$([char]9)Stood Windows$([char]9)1$([char]9)10.0.1\"); boot = (Get-Date).AddHours(-1); totalKb = 1024 }}\n\
     '@\n"
  );
  let storage = "$storage = NewAsking @'\n\
                 \"disk$([char]9)0$([char]9)Stood disk$([char]9)SSD$([char]9)NVMe$([char]9)100\"\n\
                 '@\n";
  let asking = |name: &str| Regex::new(&format!(r"(?s)\${name} = NewAsking @'\n.*?\n'@\n")).unwrap();
  let script = asking("system").replace(script, regex::NoExpand(&system)).into_owned();
  let script = asking("storage").replace(&script, regex::NoExpand(storage)).into_owned();

  assert!(script.contains("Stood Windows") && script.contains("Stood disk"));
  script
    .replace("$k % 30 -eq 0", "$k % 3 -eq 0")
    .replace("AddMilliseconds(1000)", "AddMilliseconds(100)")
}

/// The Windows side a WSL run starts, as the POSIX live script carries it.
fn wsl_side() -> String {
  let posix = include_str!("../../crates/hardware/probe-files/live-posix.sh");
  let encoded =
    Regex::new(r"-EncodedCommand ([A-Za-z0-9+/=]+)").unwrap().captures(posix).unwrap()[1].to_owned();
  let units: Vec<u16> = STANDARD
    .decode(encoded)
    .unwrap()
    .chunks(2)
    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
    .collect();

  String::from_utf16(&units).unwrap()
}

/// The blocks `pwsh` prints on `encoded` up to `count` ending with `end`, the run then killed with all it
/// started; fewer where it ends or a minute passes first.
fn blocks(encoded: &str, end: &str, count: usize) -> Vec<String> {
  let mut run = Command::new("pwsh")
    .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .process_group(0)
    .spawn()
    .unwrap();
  let pid = run.id() as i32;
  let kill = move || {
    // SAFETY: the run was started as the leader of a process group of its own.
    unsafe {
      libc::kill(-pid, libc::SIGKILL);
    }
  };
  let (done, waiting) = std::sync::mpsc::channel::<()>();
  let guard = std::thread::spawn(move || {
    if waiting.recv_timeout(Duration::from_secs(60)).is_err() {
      kill();
    }
  });
  let mut found = vec![String::new()];

  for line in BufReader::new(run.stdout.take().unwrap()).lines() {
    let Ok(line) = line else {
      break;
    };
    let last = found.last_mut().unwrap();

    last.push_str(&line);
    last.push('\n');

    if line.trim_end() == end {
      if found.len() == count {
        break;
      }

      found.push(String::new());
    }
  }

  let _ = done.send(());
  let _ = guard.join();
  kill();
  let _ = run.wait();
  found.retain(|block| block.trim_end().ends_with(end));
  found
}

#[test]
fn a_windows_run_prints_its_first_block_at_once_and_every_second_though_a_query_hangs() {
  let native = include_str!("../../crates/hardware/probe-files/live-powershell.ps1");
  let roads = [
    // A Windows node's road hands the script packed into a script block of its own.
    ("native", native.to_owned(), "@@end"),
    // A WSL run starts the Windows side on the script itself.
    ("wsl", wsl_side(), "@@winend"),
  ];

  for (name, script, end) in roads {
    let dir = tempfile::tempdir().unwrap();
    let stood = stood(&script, dir.path());
    let encoded = if name == "native" {
      encoded_command(&packed_script(&stood))
    } else {
      encoded_command(&stood)
    };
    // The first block, three readings before the system is asked again, and the readings after.
    let count = 1 + 3 + AFTER_THE_HANG;
    let found = blocks(&encoded, end, count);

    assert_eq!(found.len(), count, "{name}: {found:?}");
    assert!(!found[0].contains("@@win\n") && !found[0].contains("Stood"), "{name}: {}", found[0]);

    for reading in &found[1..] {
      assert!(
        reading.contains("os\tStood Windows") && reading.contains("disk\t0\tStood disk"),
        "{name}: {reading}"
      );
    }

    assert!(dir.path().join("hanging").exists(), "{name}: the system was not asked again");
  }
}
