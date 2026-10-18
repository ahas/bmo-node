//! A machine read once over its road: its first whole reading, what the road met where it failed, and a
//! run past its limit given up.

use std::sync::Arc;
use std::time::Duration;

use hardware::live::Road;
use hardware::once::read_once;
use hardware::roads::{Source, StreamEnd};

use crate::support::{failing_road, reading_road};

#[tokio::test]
async fn this_machine_is_read_until_its_first_whole_reading() {
  let snapshot = read_once(&reading_road(), Source::Local, Duration::from_secs(10)).await.unwrap();

  assert_eq!(snapshot.host.as_deref(), Some("box"));
  assert_eq!(snapshot.cpu_usage, Some(0.5));
}

#[tokio::test]
async fn a_road_that_fails_says_what_it_met_and_one_that_ends_early_says_so() {
  let failed = read_once(
    &failing_road("someone@192.0.2.4: Permission denied (publickey)."),
    Source::Local,
    Duration::from_secs(10),
  )
  .await;

  assert_eq!(failed.unwrap_err(), "someone@192.0.2.4: Permission denied (publickey).");

  let ended: Road = Arc::new(|_source, _script, _lines, _stop| {
    Box::pin(async {
      StreamEnd {
        ok: true,
        failure: String::new(),
      }
    })
  });

  assert_eq!(
    read_once(&ended, Source::Local, Duration::from_secs(10)).await.unwrap_err(),
    "the run ended before its first reading"
  );
}

#[tokio::test(start_paused = true)]
async fn a_read_past_its_limit_is_given_up() {
  let silent: Road = Arc::new(|_source, _script, lines, stop| {
    Box::pin(async move {
      // The run keeps its output open and says nothing.
      let _open = lines;

      stop.cancelled().await;

      StreamEnd {
        ok: false,
        failure: "stopped".into(),
      }
    })
  });

  assert_eq!(
    read_once(&silent, Source::Local, Duration::from_secs(20)).await.unwrap_err(),
    "the probe did not finish in time"
  );
}
