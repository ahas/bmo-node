//! The live runs: a reading handed on from a run's second, a run held until its stream ends, a hub's run
//! relieved before the hub stops it, a failure recorded and tried again, a run stalled before its first
//! reading stopped and tried again, a quiet run left running and said to be quiet, runs held for exactly
//! the machines given, and each road's live script.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use hardware::live::{
  Hands, QUIET, QUIET_SECONDS, RELIEF_SECONDS, RETRY_FIRST_MS, Reader, STALL_SECONDS, STALLED, Sink,
  Wanted, live_script,
};
use hardware::roads::{Source, StreamEnd};
use hardware::view::HardwareView;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// A run held open in the test's hands: what it is fed, how it ends, and whether it was stopped.
struct Held {
  lines: mpsc::Sender<String>,
  end: Option<oneshot::Sender<StreamEnd>>,
  stop: CancellationToken,
}

#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Sink for Seen {
  fn reading(&self, machine: &str, at: i64, view: &HardwareView) {
    let usage = hardware::js::round(view.cpu.usage.unwrap_or(0.0) * 100.0);

    self.0.lock().unwrap().push(format!("{machine} {at} {usage}%"));
  }

  fn failure(&self, machine: &str, at: i64, detail: &str) {
    self.0.lock().unwrap().push(format!("{machine} {at} failed: {detail}"));
  }

  fn memory(&self, machine: &str, at: i64, sample: &hardware::memory::MemorySample) {
    self
      .0
      .lock()
      .unwrap()
      .push(format!("{machine} {at} look of {}", sample.processes.len()));
  }
}

struct Stand {
  reader: Reader,
  runs: Arc<Mutex<Vec<Held>>>,
  seen: Arc<Seen>,
}

impl Stand {
  fn new() -> Self {
    let runs: Arc<Mutex<Vec<Held>>> = Arc::default();
    let seen: Arc<Seen> = Arc::default();
    let started = Instant::now();
    let made = runs.clone();
    let hands = Hands {
      stream: Arc::new(move |_source, _script, lines, stop| {
        let (end, ended) = oneshot::channel();

        made.lock().unwrap().push(Held {
          lines,
          end: Some(end),
          stop: stop.clone(),
        });

        Box::pin(async move {
          tokio::select! {
            end = ended => end.unwrap_or(StreamEnd { ok: false, failure: "gone".into() }),
            () = stop.cancelled() => StreamEnd { ok: false, failure: "stopped".into() },
          }
        })
      }),
      now_ms: Arc::new(move || 1_000_000 + started.elapsed().as_millis() as i64),
    };

    Self {
      reader: Reader::new(seen.clone(), hands),
      runs,
      seen,
    }
  }

  fn runs(&self) -> usize {
    self.runs.lock().unwrap().len()
  }

  fn stopped(&self, run: usize) -> bool {
    self.runs.lock().unwrap()[run].stop.is_cancelled()
  }

  async fn say(&self, run: usize, text: &str) {
    let lines = self.runs.lock().unwrap()[run].lines.clone();

    // A stopped run's lines go nowhere.
    for line in text.split('\n') {
      let _ = lines.send(line.to_owned()).await;
    }

    settle().await;
  }

  async fn end(&self, run: usize, end: StreamEnd) {
    let sender = self.runs.lock().unwrap()[run].end.take().unwrap();

    sender.send(end).unwrap();
    settle().await;
  }

  fn seen(&self) -> Vec<String> {
    self.seen.0.lock().unwrap().clone()
  }
}

/// Every task the step set off run as far as it goes.
async fn settle() {
  for _ in 0..20 {
    tokio::task::yield_now().await;
  }
}

async fn advance(by: Duration) {
  tokio::time::advance(by).await;
  settle().await;
}

/// One reading of the live run, the seldom parts on it where `seldom`.
fn reading(uptime: i64, busy: i64, seldom: bool) -> String {
  let mut lines: Vec<String> = Vec::new();

  if seldom {
    lines.extend(
      [
        "@@host",
        "box",
        "@@kernel",
        "6.6",
        "@@layout",
        "/sys/devices/system/cpu/cpu0/topology/core_id:0",
      ]
      .map(str::to_owned),
    );
  }

  lines.extend([
    "@@uptime".to_owned(),
    uptime.to_string(),
    "@@stat".to_owned(),
    format!("cpu  {busy} 0 0 {} 0 0 0 0", uptime * 100 - busy),
    format!("cpu0 {busy} 0 0 {} 0 0 0 0", uptime * 100 - busy),
    "@@end".to_owned(),
  ]);
  lines.join("\n")
}

fn local(name: &str) -> Wanted {
  Wanted {
    name: name.into(),
    source: Some(Source::Local),
  }
}

fn through_hub() -> Source {
  Source::Jake {
    hub: node_check::rows::Hub {
      url: "u".into(),
      token_file: "t".into(),
    },
    agent: "a".into(),
    powershell: false,
  }
}

#[tokio::test(start_paused = true)]
async fn hands_on_each_reading_from_a_runs_second_on_use_worked_out_against_the_reading_before() {
  let mut stand = Stand::new();

  stand.reader.keep(&[local("box")]);
  settle().await;
  stand.say(0, &reading(10, 0, true)).await;
  assert!(stand.seen().is_empty());
  stand.say(0, &reading(11, 50, false)).await;
  stand.say(0, &reading(12, 60, false)).await;
  assert_eq!(stand.seen(), ["box 1000 50%", "box 1001 10%"]);
}

#[tokio::test(start_paused = true)]
async fn hands_on_a_look_at_the_memory_with_the_reading_that_took_it_and_no_other() {
  let mut stand = Stand::new();
  let looking = |uptime: i64, busy: i64| {
    reading(uptime, busy, false)
      .replace("@@end", "@@procmem\n2048 0 0 7 a.service a process\n@@unitmem\n@@end")
  };

  stand.reader.keep(&[local("box")]);
  settle().await;
  stand.say(0, &reading(10, 0, true)).await;
  stand.say(0, &looking(11, 50)).await;
  stand.say(0, &reading(12, 60, false)).await;
  assert_eq!(stand.seen(), ["box 1000 50%", "box 1000 look of 1", "box 1001 10%"]);
}

#[tokio::test(start_paused = true)]
async fn holds_a_run_on_a_local_shell_or_an_ssh_login_until_its_stream_ends() {
  let mut stand = Stand::new();

  assert_eq!(Source::Local.run_seconds(), None);
  stand.reader.keep(&[local("box")]);
  settle().await;
  stand.say(0, &reading(10, 0, true)).await;
  stand.say(0, &reading(11, 10, false)).await;

  for _ in 0..2 * 60 * 6 {
    advance(Duration::from_secs(10)).await;
    stand.say(0, "@@uptime").await;
  }

  assert_eq!(stand.runs(), 1);
  assert!(!stand.stopped(0));
  stand
    .end(
      0,
      StreamEnd {
        ok: true,
        failure: String::new(),
      },
    )
    .await;
  assert_eq!(stand.runs(), 2);
}

#[tokio::test(start_paused = true)]
async fn relieves_a_hubs_run_before_the_hub_stops_it_the_next_taking_over_at_its_own_first_reading() {
  let mut stand = Stand::new();

  stand.reader.keep(&[Wanted {
    name: "box".into(),
    source: Some(through_hub()),
  }]);
  settle().await;
  stand.say(0, &reading(10, 0, true)).await;
  stand.say(0, &reading(11, 10, false)).await;

  // The run keeps talking, so only the span the hub gives it ends it.
  for _ in (0..through_hub().run_seconds().unwrap() - RELIEF_SECONDS).step_by(10) {
    advance(Duration::from_secs(10)).await;
    stand.say(0, "@@uptime").await;
  }

  assert_eq!(stand.runs(), 2);

  // The relief's start is not a reading yet, and the old run still carries the machine.
  stand.say(1, &reading(500, 0, true)).await;
  stand.say(0, &reading(12, 30, false)).await;
  stand.say(1, &reading(501, 90, false)).await;
  assert!(stand.stopped(0));
  stand.say(0, &reading(13, 40, false)).await;
  assert_eq!(
    stand
      .seen()
      .iter()
      .map(|one| one.split(' ').nth(2).unwrap().to_owned())
      .collect::<Vec<_>>(),
    ["10%", "20%", "90%"]
  );
}

#[tokio::test(start_paused = true)]
async fn records_a_failure_and_tries_again_after_a_wait_that_grows() {
  let mut stand = Stand::new();
  let first = Duration::from_millis(RETRY_FIRST_MS);

  stand.reader.keep(&[local("box")]);
  settle().await;
  stand
    .end(
      0,
      StreamEnd {
        ok: false,
        failure: "refused".into(),
      },
    )
    .await;
  assert_eq!(stand.seen(), ["box 1000 failed: refused"]);
  advance(first).await;
  assert_eq!(stand.runs(), 2);
  stand
    .end(
      1,
      StreamEnd {
        ok: true,
        failure: String::new(),
      },
    )
    .await;
  assert!(stand.seen()[1].contains("failed: the run ended before its first reading"));
  advance(first).await;
  assert_eq!(stand.runs(), 2);
  advance(first).await;
  assert_eq!(stand.runs(), 3);
}

// A run that has given no reading yet and prints nothing for a minute is stopped and fails as a run that
// ended before its first reading would; a first block, which only sets the start, is no reading.
#[tokio::test(start_paused = true)]
async fn stops_a_run_that_stalls_before_its_first_reading_and_tries_again() {
  let mut stand = Stand::new();
  let first = Duration::from_millis(RETRY_FIRST_MS);

  stand.reader.keep(&[local("box")]);
  settle().await;
  advance(Duration::from_secs(STALL_SECONDS - 1)).await;
  assert!(!stand.stopped(0));
  assert!(stand.seen().is_empty());
  advance(Duration::from_secs(1)).await;
  assert!(stand.stopped(0));
  assert_eq!(stand.seen(), [format!("box {} failed: {STALLED}", 1000 + STALL_SECONDS)]);
  advance(first).await;
  assert_eq!(stand.runs(), 2);

  stand.say(1, &reading(10, 0, true)).await;
  advance(Duration::from_secs(STALL_SECONDS)).await;
  assert!(stand.stopped(1));
  assert!(stand.seen()[1].ends_with(STALLED));
  // The wait has grown with the second failure.
  advance(first).await;
  assert_eq!(stand.runs(), 2);
  advance(first).await;
  assert_eq!(stand.runs(), 3);
}

#[tokio::test(start_paused = true)]
async fn drops_a_relief_that_stalls_while_the_run_it_was_to_relieve_still_carries_the_machine() {
  let mut stand = Stand::new();

  stand.reader.keep(&[Wanted {
    name: "box".into(),
    source: Some(through_hub()),
  }]);
  settle().await;
  stand.say(0, &reading(10, 0, true)).await;
  stand.say(0, &reading(11, 10, false)).await;

  let relief = through_hub().run_seconds().unwrap() - RELIEF_SECONDS;

  for _ in (0..relief + STALL_SECONDS as i64).step_by(10) {
    advance(Duration::from_secs(10)).await;
    stand.say(0, "@@uptime").await;
  }

  assert_eq!(stand.runs(), 2);
  assert!(stand.stopped(1));
  assert!(!stand.stopped(0));
  assert_eq!(stand.seen(), ["box 1000 10%"]);

  // The run that carried the machine is followed at its end as ever.
  stand
    .end(
      0,
      StreamEnd {
        ok: true,
        failure: String::new(),
      },
    )
    .await;
  assert_eq!(stand.runs(), 3);
}

#[tokio::test(start_paused = true)]
async fn leaves_a_run_that_falls_quiet_after_a_reading_running_and_says_so_once_for_each_quiet_stretch() {
  let mut stand = Stand::new();

  stand.reader.keep(&[local("box")]);
  settle().await;
  stand.say(0, &reading(10, 0, true)).await;
  stand.say(0, &reading(11, 50, false)).await;
  advance(Duration::from_secs(QUIET_SECONDS)).await;
  assert!(!stand.stopped(0));
  // Said at the moment the stretch is found quiet.
  assert_eq!(
    stand.seen(),
    ["box 1000 50%".to_owned(), format!("box {} failed: {QUIET}", 1000 + QUIET_SECONDS)]
  );
  advance(Duration::from_secs(10 * QUIET_SECONDS)).await;
  assert_eq!(stand.seen().len(), 2);
  assert_eq!(stand.runs(), 1);

  // A line starts a new stretch, and the readings it brings stand in the notice's place.
  stand.say(0, &reading(12, 100, false)).await;
  assert!(stand.seen()[2].ends_with("50%"));
  advance(Duration::from_secs(QUIET_SECONDS)).await;
  assert!(stand.seen()[3].ends_with(QUIET));
  assert!(!stand.stopped(0));
}

#[tokio::test(start_paused = true)]
async fn holds_runs_for_exactly_the_machines_it_is_given() {
  let mut stand = Stand::new();

  stand.reader.keep(&[
    local("a"),
    Wanted {
      name: "b".into(),
      source: None,
    },
  ]);
  settle().await;
  assert_eq!(stand.runs(), 1);
  assert_eq!(stand.seen(), ["b 1000 failed: the row is missing a field its kind needs"]);

  stand.reader.keep(&[local("a")]);
  settle().await;
  assert_eq!(stand.runs(), 1);

  stand.reader.keep(&[]);
  settle().await;
  assert!(stand.stopped(0));
}

#[test]
fn each_road_takes_the_live_script_written_for_its_language_and_how_its_runs_end() {
  let ssh = |powershell| Source::Ssh {
    target: node_check::rows::SshTarget {
      host: "h".into(),
      port: 22,
      user: "u".into(),
      key_path: None,
    },
    powershell,
  };

  // A run on a local shell or an ssh login lasts until it is stopped; a hub's ends itself inside the hub's
  // five minutes.
  for runs_on in [live_script(&Source::Local), live_script(&ssh(false))] {
    assert!(runs_on.contains("while :; do"));
    assert!(!runs_on.contains("live_end"));
  }

  assert!(live_script(&through_hub()).contains("live_end=$((live_cs + 24000))"));
  assert!(live_script(&ssh(true)).contains("while ($true) {"));
  assert!(!live_script(&ssh(true)).contains("AddSeconds"));
}
