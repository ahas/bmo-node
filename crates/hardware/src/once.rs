//! A machine read once (docs/hardware.md, 「How often」): where the recorder has nothing fresh of a tracked
//! machine, its live script is run over its own road until it has given one whole reading, and stopped,
//! or until the run ends first. A run gives its first whole reading at its second, since its first only
//! sets the start.

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::live::{Road, live_script};
use super::probe::{LiveProbe, Snapshot, read_sections};
use super::roads::Source;

/// The machine's first whole reading over its road, or what was met where the run ended before one.
pub async fn read_once(road: &Road, source: Source) -> Result<Snapshot, String> {
  let (lines, mut read) = mpsc::channel(256);
  let stop = CancellationToken::new();
  let run = tokio::spawn((road)(source.clone(), live_script(&source), lines, stop.clone()));
  let mut probe = LiveProbe::default();
  let mut block: Vec<String> = Vec::new();
  let found = async {
    while let Some(line) = read.recv().await {
      let closes = line.trim() == "@@end";

      block.push(line);

      if !closes {
        continue;
      }

      let text = format!("{}\n", block.join("\n"));

      block.clear();

      if let Some(snapshot) = probe.take(&text).as_ref().and_then(read_sections) {
        return Some(snapshot);
      }
    }

    None
  }
  .await;

  stop.cancel();

  let ended = run.await;

  match found {
    Some(snapshot) => Ok(snapshot),
    None => Err(match ended {
      Ok(end) if !end.ok && !end.failure.is_empty() => end.failure,
      _ => "the run ended before its first reading".to_owned(),
    }),
  }
}
