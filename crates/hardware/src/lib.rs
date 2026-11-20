//! Every machine's hardware read live and kept for a month (docs/hardware.md, 「Live」, 「History」 and
//! 「Tracking」): one live run of the probe held open per tracked machine, each reading written to the live
//! record in node's folder, and once a minute the runs kept to the tracked machines, the closed minutes
//! folded into the store's history, the closed half hours rolled up and what has grown old removed; and who
//! holds each machine's memory, kept beside its figures (「Who holds the memory」).

pub mod chips;
pub mod history;
pub mod hub_exec;
pub mod js;
pub mod live;
pub mod machine;
pub mod memory;
pub mod once;
pub mod probe;
pub mod record;
pub mod roads;
pub mod series;
pub mod storage;
pub mod view;
pub mod windows;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use node_check::rows::{DEFAULT_SSH_PORT, Hub, SshTarget};
use policy::nodes::{MonitoredAccounts, NodeKind};
use store::Store;
use store::nodes::Node;
use tokio::sync::Notify;

use self::live::{Hands, Sink, Wanted};
use self::record::{KEEP_SECONDS, Reader, Writer};
use self::roads::Source;
use self::view::HardwareView;

/// How often the recorder's pass runs.
pub const PASS_SECONDS: u64 = 60;

/// The name this machine is recorded under; a node is recorded under its own.
pub const LOCAL_MACHINE: &str = "local";

/// Now, in milliseconds since the epoch.
pub fn now_ms() -> i64 {
  SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as i64)
}

/// Who is told of each live reading as the recorder takes it, beside the record it is written to.
pub trait Listen: Send + Sync {
  /// One reading of a machine, at its second.
  fn reading(&self, machine: &str, at: i64, view: &HardwareView);
}

struct Recorded {
  writer: Writer,
  listen: Option<Arc<dyn Listen>>,
}

impl Sink for Recorded {
  fn reading(&self, machine: &str, at: i64, view: &HardwareView) {
    self.writer.reading(machine, at, view);

    if let Some(listen) = &self.listen {
      listen.reading(machine, at, view);
    }
  }

  fn failure(&self, machine: &str, at: i64, detail: &str) {
    self.writer.failure(machine, at, detail);
  }

  fn memory(&self, machine: &str, at: i64, sample: &memory::MemorySample) {
    self.writer.memory(machine, at, sample);
  }
}

/// The road a listed node is read by, or none where its row lacks a field its kind needs.
pub fn source_of(node: &Node) -> Option<Source> {
  match node.kind {
    NodeKind::Hub => node.hub_url.clone().zip(node.token_file.clone()).zip(node.agent.clone()).map(
      |((url, token_file), agent)| Source::Jake {
        hub: Hub { url, token_file },
        agent,
        powershell: node.powershell(),
      },
    ),
    NodeKind::Ssh => node.host.clone().zip(node.user.clone()).map(|(host, user)| Source::Ssh {
      target: SshTarget {
        host,
        port: node.port.and_then(|port| u16::try_from(port).ok()).unwrap_or(DEFAULT_SSH_PORT),
        user,
        key_path: node.key_path.clone(),
      },
      powershell: node.powershell(),
    }),
  }
}

/// One machine's stretch of the live record: its name, its figures and its looks at who holds its memory.
type HeldStretch = (String, Vec<record::Sample>, Vec<(i64, String)>);

/// The live runs and the history over node's store.
pub struct Recorder {
  store: Arc<Store>,
  record: PathBuf,
  writer: Writer,
  runs: Mutex<live::Reader>,
  /// The accounts whose personal nodes are read.
  monitored: MonitoredAccounts,
}

// The switch a tracking route moves, where a recorder stands in this process to follow it at once.
static TRACKING: OnceLock<Arc<Notify>> = OnceLock::new();

/// A machine's tracking switched: the recorder standing in this process follows it at once; where none
/// stands, the switch is read at the recorder's next pass or start (docs/hardware.md, 「Tracking」).
pub fn follow_tracking() {
  if let Some(switched) = TRACKING.get() {
    switched.notify_one();
  }
}

impl Recorder {
  /// The recorder over the store, its live record at the path given and made where it is missing, every
  /// run over the machine's own roads.
  pub fn open(store: Arc<Store>, record: &Path) -> Result<Self, String> {
    Self::with_hands(store, record, Hands::default())
  }

  pub fn with_hands(store: Arc<Store>, record: &Path, hands: Hands) -> Result<Self, String> {
    Self::listened(store, record, hands, None)
  }

  /// The recorder as `with_hands` makes it, telling `listen` of each reading it takes.
  pub fn listened(
    store: Arc<Store>,
    record: &Path,
    hands: Hands,
    listen: Option<Arc<dyn Listen>>,
  ) -> Result<Self, String> {
    let writer = Writer::open(record)?;
    let recorded = Recorded {
      writer: writer.clone(),
      listen,
    };

    Ok(Self {
      store,
      record: record.to_path_buf(),
      runs: Mutex::new(live::Reader::new(Arc::new(recorded), hands)),
      writer,
      monitored: MonitoredAccounts::default(),
    })
  }

  /// The recorder reading the personal nodes of the accounts given as well as every shared node.
  pub fn monitoring(mut self, accounts: MonitoredAccounts) -> Self {
    self.monitored = accounts;
    self
  }

  /// This machine and every monitored node the store lists, in the list's order, whose tracking is on; a
  /// personal node of an account not monitored is never read (docs/hardware.md, 「Which machines」).
  pub fn tracked(&self) -> store::Result<Vec<Wanted>> {
    let off = self.store.untracked()?;
    let mut machines = vec![Wanted {
      name: LOCAL_MACHINE.into(),
      source: Some(Source::Local),
    }];

    for node in self.store.nodes()?.into_iter().filter(|node| node.monitored(&self.monitored)) {
      machines.push(Wanted {
        source: source_of(&node),
        name: node.name,
      });
    }

    machines.retain(|machine| !off.contains(&machine.name));

    Ok(machines)
  }

  /// The live runs kept to the tracked machines: a run started for each tracked machine that has none,
  /// and the run of one no longer tracked or listed stopped.
  pub fn keep(&self) -> Result<(), String> {
    let tracked = self.tracked().map_err(|why| why.to_string())?;

    self.runs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).keep(&tracked);

    Ok(())
  }

  /// The machines whose runs stand now.
  pub fn held(&self) -> Vec<String> {
    self.runs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).held()
  }

  /// One pass: the runs kept to the tracked machines, every machine's closed minutes folded into the
  /// history, the closed half hours rolled up, and what has grown older than either keeps removed.
  pub fn pass(&self, now: i64) -> Result<(), String> {
    let failed = |why: store::StoreError| why.to_string();

    self.keep()?;

    // The record's stretch read whole first: every minute a fold can reach lies within it.
    let held: Vec<HeldStretch> = match Reader::open(&self.record) {
      None => vec![],
      Some(reader) => reader
        .machines()
        .and_then(|machines| {
          machines
            .into_iter()
            .map(|machine| {
              let from = now - KEEP_SECONDS - history::MINUTE_SECONDS;
              let samples = reader.samples(&machine, from, now + 1)?;
              let looks = reader.memory_samples(&machine, from, now + 1)?;

              Ok((machine, samples, looks))
            })
            .collect()
        })
        .map_err(|why| why.to_string())?,
    };

    for (machine, samples, looks) in held {
      history::fold(&self.store, &machine, now, KEEP_SECONDS, |from, to| {
        samples.into_iter().filter(|one| one.at >= from && one.at < to).collect()
      })
      .map_err(failed)?;
      memory::fold(&self.store, &machine, now, KEEP_SECONDS, looks).map_err(failed)?;
    }

    history::roll_up(&self.store, now, history::ROLLUP_BATCH).map_err(failed)?;
    history::prune(&self.store, now).map_err(failed)?;
    memory::prune(&self.store, now).map_err(failed)?;
    self.writer.prune(now, KEEP_SECONDS);

    Ok(())
  }

  /// The recorder following every tracking switch at once, for as long as the process runs.
  pub fn follow(self: &Arc<Self>) {
    let switched = TRACKING.get_or_init(|| Arc::new(Notify::new())).clone();
    let recorder = Arc::downgrade(self);

    tokio::spawn(async move {
      loop {
        switched.notified().await;

        let Some(recorder) = recorder.upgrade() else {
          return;
        };

        if let Err(why) = recorder.keep() {
          eprintln!("bmo-node: following a tracking switch: {why}");
        }
      }
    });
  }

  pub fn stop(&self) {
    self.runs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).stop();
  }
}
