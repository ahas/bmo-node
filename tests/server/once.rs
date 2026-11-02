//! A machine read once over its road: its first whole reading, what the road met where it failed, and a
//! quiet run waited on until it ends.

use std::sync::Arc;
use std::time::Duration;

use hardware::live::Road;
use hardware::once::read_once;
use hardware::roads::{Source, StreamEnd};

use crate::server::support::{failing_road, reading_road};

#[tokio::test]
async fn this_machine_is_read_until_its_first_whole_reading() {
  let snapshot = read_once(&reading_road(), Source::Local).await.unwrap();

  assert_eq!(snapshot.host.as_deref(), Some("box"));
  assert_eq!(snapshot.cpu_usage, Some(0.5));
}

#[tokio::test]
async fn a_road_that_fails_says_what_it_met_and_one_that_ends_early_says_so() {
  let failed =
    read_once(&failing_road("someone@192.0.2.4: Permission denied (publickey)."), Source::Local).await;

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
    read_once(&ended, Source::Local).await.unwrap_err(),
    "the run ended before its first reading"
  );
}

#[tokio::test(start_paused = true)]
async fn a_quiet_run_is_waited_on_until_it_ends() {
  let quiet: Road = Arc::new(|_source, _script, lines, _stop| {
    Box::pin(async move {
      // The run keeps its output open and says nothing for an hour, then its login breaks.
      let _open = lines;

      tokio::time::sleep(Duration::from_secs(60 * 60)).await;

      StreamEnd {
        ok: false,
        failure: "Connection to 192.0.2.4 closed by remote host.".into(),
      }
    })
  });

  assert_eq!(
    read_once(&quiet, Source::Local).await.unwrap_err(),
    "Connection to 192.0.2.4 closed by remote host."
  );
}
