//! The recorder keeping its live runs to the tracked machines over node's store, and the live record
//! keeping each machine's latest and its figures and pruning by reach.

use std::sync::Arc;

use hardware::live::Hands;
use hardware::record::{KEEP_SECONDS, Reader, Writer};
use hardware::roads::{Source, StreamEnd};
use hardware::{Recorder, probe, view};
use store::Store;
use store::nodes::SshLogin;

use crate::fixtures;

/// Runs that print nothing and end only when stopped: a recorder whose roads reach no machine.
fn quiet_runs() -> Hands {
  Hands {
    stream: Arc::new(|_source, _script, _lines, stop| {
      Box::pin(async move {
        stop.cancelled().await;
        StreamEnd {
          ok: false,
          failure: "stopped".into(),
        }
      })
    }),
    now_ms: Arc::new(|| 1_000_000),
  }
}

#[tokio::test]
async fn the_recorder_holds_live_runs_for_the_tracked_machines_alone_following_a_switch_when_kept_again() {
  let dir = tempfile::tempdir().unwrap();
  let store = Arc::new(Store::in_memory().unwrap());
  let recorder =
    Recorder::with_hands(store.clone(), &dir.path().join("hardware-live.db"), quiet_runs()).unwrap();

  store
    .add_node(
      "pc-node",
      Some("PC"),
      None,
      SshLogin {
        shell: "powershell".into(),
        ..SshLogin::new("192.0.2.82", "tester")
      },
    )
    .unwrap();
  // A jake node whose hub is not listed has no token file, so no road reaches it.
  store.add_agent("http://hub.example", "half").unwrap();

  let tracked = recorder.tracked().unwrap();

  assert_eq!(
    tracked.iter().map(|one| one.name.as_str()).collect::<Vec<_>>(),
    ["local", "pc-node", "half"]
  );
  assert!(matches!(
    &tracked[1].source,
    Some(Source::Ssh {
      powershell: true,
      ..
    })
  ));
  assert!(tracked[2].source.is_none());

  recorder.keep().unwrap();
  assert_eq!(recorder.held(), ["local", "pc-node"]);

  store.set_tracking("pc-node", false).unwrap();
  recorder.keep().unwrap();
  assert_eq!(recorder.held(), ["local"]);

  store.set_tracking("pc-node", true).unwrap();
  store.set_tracking("local", false).unwrap();
  recorder.keep().unwrap();
  assert_eq!(recorder.held(), ["pc-node"]);
  recorder.stop();
}

#[tokio::test]
async fn the_live_record_keeps_each_machines_latest_and_its_figures_and_prunes_by_reach_and_by_size() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("hardware-live.db");
  let writer = Writer::open(&path).unwrap();
  let text = std::fs::read_to_string(fixtures().join("linux.txt")).unwrap();
  let view = view::hardware_view(&probe::read_sections(&probe::sections(&text)).unwrap());

  // A reader finds the record from the start, made where it was missing.
  assert!(Reader::open(&path).is_some());

  writer.reading("box", 100, &view);
  writer.reading("box", 101, &view);
  writer.failure("box", 102, "the machine stopped answering");
  writer.written();

  let reader = Reader::open(&path).unwrap();
  let latest = reader.latest("box").unwrap().unwrap();

  assert_eq!(
    (latest.at, latest.view, latest.failure.as_deref()),
    (102, None, Some("the machine stopped answering"))
  );
  assert_eq!(reader.machines().unwrap(), ["box"]);

  let samples = reader.samples("box", 0, 1_000).unwrap();

  assert_eq!(samples.iter().map(|one| one.at).collect::<Vec<_>>(), [100, 101]);
  assert_eq!(samples[0].sample["cpu|use"], 50);
  assert_eq!(samples[0].sample["gpu0:vram|gb"], 0.03);

  // Older than the reach goes first.
  writer.prune(101 + KEEP_SECONDS, KEEP_SECONDS);
  writer.written();
  assert_eq!(
    reader.samples("box", 0, 1_000).unwrap().iter().map(|one| one.at).collect::<Vec<_>>(),
    [101]
  );
  assert!(Reader::open(&dir.path().join("none.db")).is_none());
}

#[tokio::test]
async fn a_pass_folds_the_live_record_into_the_store_s_history() {
  let dir = tempfile::tempdir().unwrap();
  let store = Arc::new(Store::in_memory().unwrap());
  let path = dir.path().join("hardware-live.db");
  let recorder = Recorder::with_hands(store.clone(), &path, quiet_runs()).unwrap();
  let writer = Writer::open(&path).unwrap();
  let text = std::fs::read_to_string(fixtures().join("linux.txt")).unwrap();
  let view = view::hardware_view(&probe::read_sections(&probe::sections(&text)).unwrap());
  let now = 1_800_000_000;

  writer.reading("box", now - 70, &view);
  writer.reading("box", now - 65, &view);
  writer.written();
  recorder.pass(now).unwrap();
  recorder.stop();

  let rows = store.samples("box", 0, now).unwrap();

  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].at, (now - 70).div_euclid(60) * 60);
}
