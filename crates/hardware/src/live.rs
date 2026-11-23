//! Every machine read live (docs/hardware.md, 「Live」): one run of the probe's live form held open per
//! machine until its stream ends, each reading handed to the record as it lands. A run on a road that
//! stops runs itself is relieved before then by the next, which takes over at its own first reading; a
//! run that fails or ends before its first reading has its failure recorded, and another is started after
//! a wait that grows while the failures go on. A run that falls quiet before its first reading is stopped
//! and fails as one that ended there; one that falls quiet after it is left running, and the machine's
//! latest says so.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::js::number;
use super::memory::{MemorySample, holders, sample_of};
use super::probe::{LiveProbe, read_sections};
use super::roads::{Source, StreamEnd, stream};
use super::view::{HardwareView, hardware_view};

/// How far ahead of the end a road gives a run the next is started: past the slowest start of a run, a
/// WSL machine's counters and sensor library included.
pub const RELIEF_SECONDS: i64 = 20;

/// How long a run that has given a reading goes without printing a line before the machine's latest says
/// it has gone quiet.
pub const QUIET_SECONDS: u64 = 30;

/// What the machine's latest says of a run gone quiet.
pub const QUIET: &str = "the machine has printed nothing for thirty seconds";

/// How long a run that has given no reading yet goes without printing a line before it is stopped: past
/// the slowest healthy start of a run, whose script prints its first block before anything slow.
pub const STALL_SECONDS: u64 = 60;

/// What the machine's latest says of a run stopped before its first reading.
pub const STALLED: &str = "the run printed nothing for a minute before its first reading";

/// The waits before a failed machine is tried again: the first, and the longest it grows to.
pub const RETRY_FIRST_MS: u64 = 1_000;
pub const RETRY_MOST_MS: u64 = 30_000;

/// How far a reading's second may stray from this machine's clock before the run's seconds are counted
/// afresh from now.
const ANCHOR_DRIFT_SECONDS: i64 = 5;

// The probe's live form as each road takes it, written out whole under the crate's `probe-files/`, which
// is the one source of the script (docs/hardware.md, 「The probe」).
const LIVE_POSIX: &str = include_str!("../probe-files/live-posix.sh");
const LIVE_POSIX_HUB: &str = include_str!("../probe-files/live-posix-240.sh");
const LIVE_POWERSHELL: &str = include_str!("../probe-files/live-powershell.ps1");
const LIVE_POWERSHELL_HUB: &str = include_str!("../probe-files/live-powershell-240.ps1");

/// The live script a road takes: one that ends itself inside the span the road gives a run, or one that
/// runs until it is stopped.
pub fn live_script(source: &Source) -> &'static str {
  match source {
    Source::Jake {
      powershell: true, ..
    } => LIVE_POWERSHELL_HUB,
    Source::Jake { .. } => LIVE_POSIX_HUB,
    Source::Ssh {
      powershell: true, ..
    } => LIVE_POWERSHELL,
    _ => LIVE_POSIX,
  }
}

/// Where the readings and failures go.
pub trait Sink: Send + Sync {
  fn reading(&self, machine: &str, at: i64, view: &HardwareView);
  fn failure(&self, machine: &str, at: i64, detail: &str);
  /// A look at who holds the machine's memory, taken with the reading of the same second.
  fn memory(&self, _machine: &str, _at: i64, _sample: &MemorySample) {}
}

pub type Streaming = Pin<Box<dyn Future<Output = StreamEnd> + Send>>;

/// A live script held open on a machine's road, its lines handed to the sender, until it ends or is
/// stopped.
pub type Road =
  Arc<dyn Fn(Source, &'static str, mpsc::Sender<String>, CancellationToken) -> Streaming + Send + Sync>;

/// What a run is held open with, which a test stands in for: the road, and this machine's clock in
/// milliseconds.
#[derive(Clone)]
pub struct Hands {
  pub stream: Road,
  pub now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl Default for Hands {
  fn default() -> Self {
    Self {
      stream: Arc::new(|source, script, lines, stop| {
        Box::pin(async move { stream(&source, script, lines, stop).await })
      }),
      now_ms: Arc::new(crate::now_ms),
    }
  }
}

enum Event {
  Line(u64, String),
  Ended(u64, StreamEnd),
}

struct Run {
  id: u64,
  stop: CancellationToken,
  probe: LiveProbe,
  lines: Vec<String>,
  published: bool,
  relief_at: Option<Instant>,
  /// When the run will have gone quiet, none once the machine's latest has said so for this stretch.
  quiet_at: Option<Instant>,
  /// The second a reading of the run was taken at, as this machine's clock said it, and the machine's
  /// own uptime then.
  anchor: Option<(i64, f64)>,
}

impl Run {
  /// When the run will have gone quiet if it prints nothing from now: later while it has given no reading,
  /// since it is still starting.
  fn quiet_from_now(&self) -> Instant {
    let seconds = if self.published { QUIET_SECONDS } else { STALL_SECONDS };

    Instant::now() + Duration::from_secs(seconds)
  }
}

/// One machine's runs: the one carrying it, and the one started to relieve it.
struct Machine {
  name: String,
  source: Source,
  sink: Arc<dyn Sink>,
  hands: Hands,
  events: mpsc::Sender<Event>,
  current: Option<Run>,
  next: Option<Run>,
  failures: u32,
  retry_at: Option<Instant>,
  made: u64,
}

impl Machine {
  fn at(&self) -> i64 {
    (self.hands.now_ms)().div_euclid(1000)
  }

  fn launch(&mut self) {
    let now = Instant::now();
    let lifetime = self.source.run_seconds();

    self.made += 1;

    let run = Run {
      id: self.made,
      stop: CancellationToken::new(),
      probe: LiveProbe::default(),
      lines: vec![],
      published: false,
      relief_at: lifetime
        .map(|lifetime| now + Duration::from_secs((lifetime - RELIEF_SECONDS).max(0) as u64)),
      quiet_at: Some(now + Duration::from_secs(STALL_SECONDS)),
      anchor: None,
    };
    let (lines, mut heard) = mpsc::channel::<String>(64);
    let events = self.events.clone();
    let id = run.id;
    let mut streaming =
      (self.hands.stream)(self.source.clone(), live_script(&self.source), lines, run.stop.clone());

    // Each line handed on as it comes, and the run's end once it has ended, after every line it printed.
    tokio::spawn(async move {
      let end = loop {
        tokio::select! {
          biased;
          line = heard.recv() => match line {
            Some(line) => {
              if events.send(Event::Line(id, line)).await.is_err() {
                return;
              }
            }
            None => break streaming.await,
          },
          end = &mut streaming => break end,
        }
      };

      while let Ok(line) = heard.try_recv() {
        if events.send(Event::Line(id, line)).await.is_err() {
          return;
        }
      }

      let _ = events.send(Event::Ended(id, end)).await;
    });

    if self.current.is_none() {
      self.current = Some(run);
    } else {
      self.next = Some(run);
    }
  }

  fn run_mut(&mut self, id: u64) -> Option<&mut Run> {
    [self.current.as_mut(), self.next.as_mut()]
      .into_iter()
      .flatten()
      .find(|run| run.id == id)
  }

  fn line(&mut self, id: u64, line: String) {
    let Some(run) = self.run_mut(id) else {
      return;
    };

    if run.stop.is_cancelled() {
      return;
    }

    run.quiet_at = Some(run.quiet_from_now());

    if line.trim() != "@@end" {
      run.lines.push(line);
      return;
    }

    let text = format!("{}\n@@end\n", run.lines.join("\n"));

    run.lines.clear();

    let Some(whole) = run.probe.take(&text) else {
      return;
    };
    let Some(snapshot) = read_sections(&whole) else {
      return;
    };
    let view = hardware_view(&snapshot);
    let uptime = whole.get("uptime").and_then(|lines| lines.first()).and_then(|line| number(line));
    let at = self.second(id, uptime);
    let sample =
      holders(&whole).map(|(processes, units)| sample_of(&self.name, at, &view, processes, units));

    self.publish(id, &view, sample.as_ref(), at);
  }

  /// The second a reading was taken at: counted along the machine's own uptime from the run's anchor,
  /// so readings a second apart on the machine stand a second apart in the record however unevenly the
  /// road brings them.
  fn second(&mut self, id: u64, uptime: Option<f64>) -> i64 {
    let now = self.at();
    let Some(run) = self.run_mut(id) else {
      return now;
    };
    let Some(uptime) = uptime.filter(|uptime| uptime.is_finite()) else {
      return now;
    };
    let at = run.anchor.map_or(now, |(at, from)| at + super::js::round(uptime - from) as i64);

    if run.anchor.is_none() || (at - now).abs() > ANCHOR_DRIFT_SECONDS {
      run.anchor = Some((now, uptime));

      return now;
    }

    at
  }

  fn publish(&mut self, id: u64, view: &HardwareView, sample: Option<&MemorySample>, at: i64) {
    if self.next.as_ref().is_some_and(|run| run.id == id) {
      let relieved = self.current.take();

      self.current = self.next.take();

      if let Some(relieved) = relieved {
        relieved.stop.cancel();
      }
    }

    let Some(run) = self.current.as_mut().filter(|run| run.id == id) else {
      return;
    };

    run.published = true;
    run.quiet_at = Some(run.quiet_from_now());
    self.failures = 0;
    self.sink.reading(&self.name, at, view);

    if let Some(sample) = sample {
      self.sink.memory(&self.name, at, sample);
    }
  }

  fn ended(&mut self, id: u64, end: StreamEnd) {
    let run = if self.current.as_ref().is_some_and(|run| run.id == id) {
      let run = self.current.take();

      self.current = self.next.take();
      run
    } else if self.next.as_ref().is_some_and(|run| run.id == id) {
      self.next.take()
    } else {
      // A run already relieved or stopped: its ending says nothing about the machine.
      return;
    };
    let Some(run) = run else {
      return;
    };

    run.stop.cancel();

    if self.current.is_some() {
      return;
    }

    // A run that carried the machine to its end is simply followed; anything else is a failure the
    // machine's latest says.
    if end.ok && run.published {
      self.launch();
      return;
    }

    self.failures += 1;

    let detail = if end.ok { "the run ended before its first reading" } else { end.failure.as_str() };

    self.sink.failure(&self.name, self.at(), detail);

    let wait = RETRY_MOST_MS.min(RETRY_FIRST_MS.saturating_mul(1 << (self.failures - 1).min(16)));

    self.retry_at = Some(Instant::now() + Duration::from_millis(wait));
  }

  /// The earliest moment a timer of the machine's comes due.
  fn due(&self) -> Option<Instant> {
    [self.current.as_ref(), self.next.as_ref()]
      .into_iter()
      .flatten()
      .flat_map(|run| [run.relief_at, run.quiet_at])
      .chain([self.retry_at])
      .flatten()
      .min()
  }

  fn timers(&mut self) {
    let now = Instant::now();

    if self.retry_at.is_some_and(|at| at <= now) {
      self.retry_at = None;

      if self.current.is_none() {
        self.launch();
      }
    }

    for id in [self.current.as_ref(), self.next.as_ref()]
      .into_iter()
      .flatten()
      .map(|run| run.id)
      .collect::<Vec<_>>()
    {
      let carrying = self.current.as_ref().is_some_and(|current| current.id == id);
      let Some(run) = self.run_mut(id) else {
        continue;
      };
      let quiet = run.quiet_at.is_some_and(|at| at <= now);
      let relieved = run.relief_at.is_some_and(|at| at <= now);
      let published = run.published;

      if quiet {
        run.quiet_at = None;
      }

      if relieved {
        run.relief_at = None;
      }

      // A run that never got going is stopped, and fails as a run that ended before its first reading.
      if quiet && !published {
        self.ended(
          id,
          StreamEnd {
            ok: false,
            failure: STALLED.into(),
          },
        );
        continue;
      }

      // Only the run carrying the machine speaks for it, and it goes on running.
      if quiet && carrying {
        self.sink.failure(&self.name, self.at(), QUIET);
      }

      if relieved && carrying && self.next.is_none() {
        self.launch();
      }
    }
  }

  fn stop(&mut self) {
    for run in [self.current.take(), self.next.take()].into_iter().flatten() {
      run.stop.cancel();
    }
  }
}

/// One machine held open until `stop`.
async fn hold(mut machine: Machine, mut events: mpsc::Receiver<Event>, stop: CancellationToken) {
  machine.launch();

  loop {
    let due = machine.due();

    tokio::select! {
      () = stop.cancelled() => break,
      event = events.recv() => match event {
        Some(Event::Line(id, line)) => machine.line(id, line),
        Some(Event::Ended(id, end)) => machine.ended(id, end),
        None => break,
      },
      () = async { tokio::time::sleep_until(due.unwrap_or_else(Instant::now)).await }, if due.is_some() => machine.timers(),
    }
  }

  machine.stop();
}

/// A machine to hold open, by the name the monitor knows it by: this one's `local`, a node's own name.
/// None for a row missing a field its kind needs.
pub struct Wanted {
  pub name: String,
  pub source: Option<Source>,
}

struct Held {
  road: String,
  stop: CancellationToken,
  task: JoinHandle<()>,
}

/// Every machine's runs: held open for exactly the machines it is given, a new one started and one gone
/// or whose road changed stopped.
pub struct Reader {
  sink: Arc<dyn Sink>,
  hands: Hands,
  held: BTreeMap<String, Held>,
}

impl Reader {
  pub fn new(sink: Arc<dyn Sink>, hands: Hands) -> Self {
    Self {
      sink,
      hands,
      held: BTreeMap::new(),
    }
  }

  pub fn keep(&mut self, machines: &[Wanted]) {
    self.held.retain(|name, held| {
      let road = machines
        .iter()
        .find(|one| one.name == *name)
        .and_then(|one| one.source.as_ref())
        .map(Source::road);
      let kept = road.as_ref() == Some(&held.road);

      if !kept {
        held.stop.cancel();
      }

      kept
    });

    for one in machines {
      let Some(source) = &one.source else {
        self.sink.failure(
          &one.name,
          (self.hands.now_ms)().div_euclid(1000),
          "the row is missing a field its kind needs",
        );
        continue;
      };

      if self.held.contains_key(&one.name) {
        continue;
      }

      let (events, heard) = mpsc::channel(256);
      let stop = CancellationToken::new();
      let machine = Machine {
        name: one.name.clone(),
        source: source.clone(),
        sink: self.sink.clone(),
        hands: self.hands.clone(),
        events,
        current: None,
        next: None,
        failures: 0,
        retry_at: None,
        made: 0,
      };

      self.held.insert(
        one.name.clone(),
        Held {
          road: source.road(),
          stop: stop.clone(),
          task: tokio::spawn(hold(machine, heard, stop)),
        },
      );
    }
  }

  /// The machines held open, by name.
  pub fn held(&self) -> Vec<String> {
    self.held.keys().cloned().collect()
  }

  pub fn stop(&mut self) {
    for (_, held) in std::mem::take(&mut self.held) {
      held.stop.cancel();
      drop(held.task);
    }
  }
}

impl Drop for Reader {
  fn drop(&mut self) {
    self.stop();
  }
}
