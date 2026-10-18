//! A machine's own road: a local run hands on each line and says how it ended, and a run stopped is ended
//! at once.

use hardware::roads::{Source, StreamEnd, stream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn a_local_run_hands_on_each_line_and_says_how_it_ended() {
  let (lines, mut read) = mpsc::channel(16);
  let end = stream(
    &Source::Local,
    "printf 'one\\r\\ntwo\\nthree'; echo gone >&2; exit 3",
    lines,
    CancellationToken::new(),
  )
  .await;
  let mut got = Vec::new();

  while let Ok(line) = read.try_recv() {
    got.push(line);
  }

  assert_eq!(got, ["one", "two", "three"]);
  assert_eq!(
    end,
    StreamEnd {
      ok: false,
      failure: "gone".into()
    }
  );
}

#[tokio::test]
async fn a_run_stopped_is_ended_at_once() {
  let (lines, _read) = mpsc::channel(16);
  let stop = CancellationToken::new();

  stop.cancel();

  let end = stream(&Source::Local, "exec sleep 600", lines, stop).await;

  assert!(!end.ok);
}
