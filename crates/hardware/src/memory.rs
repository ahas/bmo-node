//! Who holds a machine's memory (docs/hardware.md, 「Who holds the memory」): the looks a live run prints every few
//! readings, read into a sample with the machine's figures at that reading; each closed minute's worst folded out
//! of the live record into the store; and a machine's samples read back, the live record's where it reaches and the
//! store's before that.

use std::collections::BTreeMap;

pub use bmo_node_client::hardware::{MemorySample, MemorySamplePage, ProcessMemory, UnitMemory};
use store::Store;

use super::history::MINUTE_SECONDS;
use super::js::round_to;
use super::probe::Sections;
use super::record::{KEEP_SECONDS as RECORD_KEEP_SECONDS, Reader};
use super::series::MS_PER_SECOND;
use super::view::HardwareView;

/// How long a minute's sample is kept: past any exhaustion looked into in the days after it.
pub const KEEP_SECONDS: i64 = 7 * 86_400;

/// The most samples a page holds where none is asked for.
pub const DEFAULT_LIMIT: usize = 500;

const KB_PER_MB: f64 = 1024.0;

fn megabytes(kilobytes: &str) -> Option<i64> {
  Some((kilobytes.parse::<f64>().ok()? / KB_PER_MB).round() as i64)
}

/// A process's line: its resident memory, `RssShmem` and swap in kilobytes, its pid, its unit or `-`, and its name,
/// which may hold spaces.
fn process(line: &str) -> Option<ProcessMemory> {
  let mut fields = line.trim().splitn(6, ' ');
  let (rss, shmem, swap) = (fields.next()?, fields.next()?, fields.next()?);
  let pid = fields.next()?.parse().ok()?;
  let unit = fields.next()?;

  Some(ProcessMemory {
    pid,
    unit: (unit != "-").then(|| unit.to_owned()),
    name: fields.next()?.to_owned(),
    rss_mb: megabytes(rss)?,
    shmem_mb: megabytes(shmem)?,
    swap_mb: megabytes(swap)?,
  })
}

/// A unit's line: its current memory and swap together, `anon`, `shmem`, `file` and swap in kilobytes, and its name.
fn unit(line: &str) -> Option<UnitMemory> {
  let fields: Vec<&str> = line.split_whitespace().collect();
  let &[total, anon, shmem, file, swap, unit] = fields.as_slice() else {
    return None;
  };
  let swap_mb = megabytes(swap)?;

  Some(UnitMemory {
    unit: unit.to_owned(),
    current_mb: megabytes(total)? - swap_mb,
    anon_mb: megabytes(anon)?,
    shmem_mb: megabytes(shmem)?,
    file_mb: megabytes(file)?,
    swap_mb,
  })
}

/// The look a reading carries, none where this reading took none; a line that does not read is left out.
pub fn holders(found: &Sections) -> Option<(Vec<ProcessMemory>, Vec<UnitMemory>)> {
  let processes = found.get("procmem")?.iter().filter_map(|line| process(line)).collect();
  let units = found.get("unitmem").into_iter().flatten().filter_map(|line| unit(line)).collect();

  Some((processes, units))
}

/// A look as the sample it is, with the machine's figures at the same reading.
pub fn sample_of(
  machine: &str,
  at_seconds: i64,
  view: &HardwareView,
  processes: Vec<ProcessMemory>,
  units: Vec<UnitMemory>,
) -> MemorySample {
  let gigabytes = |kb: Option<f64>| kb.map(|kb| round_to(kb / 1024.0 / 1024.0, 100.0));

  MemorySample {
    machine_name: machine.to_owned(),
    at: at_seconds * MS_PER_SECOND,
    available_gb: gigabytes(
      view.memory.total_kb.zip(view.memory.used_kb).map(|(total, used)| total - used),
    ),
    shmem_gb: gigabytes(view.memory.shmem_kb),
    swap_used_gb: gigabytes(view.memory.swap_used_kb),
    processes,
    units,
  }
}

/// How little memory a sample's text says stood available; a sample that says nothing stands last.
fn available_in(text: &str) -> f64 {
  serde_json::from_str::<MemorySample>(text)
    .ok()
    .and_then(|sample| sample.available_gb)
    .unwrap_or(f64::INFINITY)
}

/// Every minute closed by `now` since the machine's last sample, as far back as the live record reaches, folded out
/// of the looks handed: the look of the lowest `MemAvailable` of each, under its own second, the earliest among
/// equals. A minute with no look leaves no row. How many rows were written.
pub fn fold(
  store: &Store,
  machine: &str,
  now: i64,
  reach: i64,
  looks: Vec<(i64, String)>,
) -> store::Result<usize> {
  let last = store.last_memory_sample_at(machine)?;
  let closed = now.div_euclid(MINUTE_SECONDS) * MINUTE_SECONDS;
  let after_last = last.map_or(0, |last| last.div_euclid(MINUTE_SECONDS) * MINUTE_SECONDS + MINUTE_SECONDS);
  let from = after_last.max((now - reach).div_euclid(MINUTE_SECONDS) * MINUTE_SECONDS);
  let mut minutes: BTreeMap<i64, (f64, i64, String)> = BTreeMap::new();

  for (at, text) in looks {
    if at < from || at >= closed {
      continue;
    }

    let available = available_in(&text);
    let minute = at.div_euclid(MINUTE_SECONDS) * MINUTE_SECONDS;

    if minutes.get(&minute).is_none_or(|worst| available < worst.0) {
      minutes.insert(minute, (available, at, text));
    }
  }

  let rows: Vec<(i64, String)> = minutes.into_values().map(|(_, at, text)| (at, text)).collect();

  store.put_memory_samples(machine, &rows)?;

  Ok(rows.len())
}

/// Samples past the week kept removed; how many went.
pub fn prune(store: &Store, now: i64) -> store::Result<usize> {
  store.prune_memory_samples(now - KEEP_SECONDS)
}

/// Why a machine's samples could not be read.
#[derive(Debug)]
pub enum SamplesError {
  Store(store::StoreError),
  Record(rusqlite::Error),
}

impl std::fmt::Display for SamplesError {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Store(why) => write!(out, "{why}"),
      Self::Record(why) => write!(out, "the live record could not be read: {why}"),
    }
  }
}

/// Which of a machine's samples to read: the first instant after which a sample was taken and the instant before
/// which it was, in milliseconds, and how many at most.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AskedSamples {
  pub after_ms: Option<i64>,
  pub before_ms: Option<i64>,
  pub limit: Option<usize>,
}

/// The machine's samples asked for at `now`, in seconds, oldest first: the live record's within its reach, the
/// store's before it, at most `limit` of them and whether more stand past the last.
pub fn read_samples(
  store: &Store,
  live: Option<&Reader>,
  machine: &str,
  asked: AskedSamples,
  now: i64,
) -> Result<MemorySamplePage, SamplesError> {
  let reach = now - KEEP_SECONDS;
  let from = asked.after_ms.map_or(reach, |after| after.div_euclid(MS_PER_SECOND) + 1).max(reach);
  let until = asked
    .before_ms
    .map_or(now + 1, |before| (before + MS_PER_SECOND - 1).div_euclid(MS_PER_SECOND));
  let live_from = live.map_or(until, |_| now - RECORD_KEEP_SECONDS);
  let kept = store.memory_samples(machine, from, until.min(live_from)).map_err(SamplesError::Store)?;
  let held = match live {
    Some(reader) => reader
      .memory_samples(machine, from.max(live_from), until)
      .map_err(SamplesError::Record)?,
    None => Vec::new(),
  };
  let limit = asked.limit.unwrap_or(DEFAULT_LIMIT);
  let texts = kept.into_iter().map(|row| row.figures).chain(held.into_iter().map(|(_, text)| text));
  let mut items: Vec<MemorySample> = texts
    .filter_map(|text| serde_json::from_str(&text).ok())
    .take(limit.saturating_add(1))
    .collect();
  let more = items.len() > limit;

  items.truncate(limit);

  Ok(MemorySamplePage { items, more })
}
