//! A machine's recorded figures as readings (docs/hardware.md, 「History」; docs/api.md, 「Machines」): the
//! stretches of one span's width, each drawn out of the finest rows that hold it, each figure its lowest,
//! highest and mean over the stretch, laid out per piece of hardware.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use store::Store;

pub use bmo_node_client::hardware::{
  Band, CpuFigures, FanFigures, GpuFigures, MemoryFigures, Reading, ReadingPage, StorageFigures,
};

use super::history::{MINUTE_SECONDS, ROLLUP_KEEP_SECONDS, ROLLUP_SECONDS, SAMPLE_KEEP_SECONDS, parsed};
use super::js::round_to;
use super::record::{KEEP_SECONDS, Reader};

/// Milliseconds in a second: a reading answers its instants and its span in milliseconds, and the record and
/// the store keep them in seconds.
pub const MS_PER_SECOND: i64 = 1_000;

/// The span a reading is read at where none is asked: one live reading.
pub const DEFAULT_SPAN_MS: i64 = MS_PER_SECOND;

/// The rows a span's stretches are drawn out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rows {
  /// The live record, one row a second.
  Live,
  /// The minutes' rows.
  Minutes,
  /// The half hours' rows, with the minutes' rows of a half hour not rolled up yet.
  HalfHours,
}

impl Rows {
  /// The rows a span is drawn out of, none for a span no rows hold whole: whole seconds under a minute,
  /// whole minutes from a minute up, and the half hours' rows for whole half hours.
  pub fn of_span(span_ms: i64) -> Option<Self> {
    if span_ms <= 0 || span_ms % MS_PER_SECOND != 0 {
      return None;
    }

    let seconds = span_ms / MS_PER_SECOND;

    if seconds < MINUTE_SECONDS {
      Some(Self::Live)
    } else if seconds % ROLLUP_SECONDS == 0 {
      Some(Self::HalfHours)
    } else if seconds % MINUTE_SECONDS == 0 {
      Some(Self::Minutes)
    } else {
      None
    }
  }

  /// How far back the rows reach, in seconds.
  pub fn reach_seconds(self) -> i64 {
    match self {
      Self::Live => KEEP_SECONDS,
      Self::Minutes => SAMPLE_KEEP_SECONDS,
      Self::HalfHours => ROLLUP_KEEP_SECONDS,
    }
  }
}

/// A figure over a stretch: its lowest, its highest, the sum and how many readings.
type Tally = [f64; 4];

fn tally_of(value: &Value) -> Option<Tally> {
  let values = value.as_array().filter(|values| values.len() == 4)?;
  let read: Vec<f64> = values.iter().map(Value::as_f64).collect::<Option<_>>()?;

  Some([read[0], read[1], read[2], read[3]])
}

/// Tallies gathered into stretches: every reading or row adds to the stretch its instant falls in.
struct Stretches {
  count: usize,
  by_index: BTreeMap<usize, BTreeMap<String, Tally>>,
}

impl Stretches {
  fn new(count: usize) -> Self {
    Self {
      count,
      by_index: BTreeMap::new(),
    }
  }

  fn add(&mut self, index: i64, key: &str, [min, max, sum, count]: Tally) {
    if index < 0 || index as usize >= self.count || count <= 0.0 {
      return;
    }

    let tallies = self.by_index.entry(index as usize).or_default();
    let known = tallies.entry(key.to_owned()).or_insert([min, max, 0.0, 0.0]);

    *known = [known[0].min(min), known[1].max(max), known[2] + sum, known[3] + count];
  }

  /// A row's figures, each a single reading or a tally of several.
  fn add_sample(&mut self, index: i64, sample: &Map<String, Value>) {
    for (key, value) in sample {
      match value.as_f64().filter(|value| value.is_finite()) {
        Some(reading) => self.add(index, key, [reading, reading, reading, 1.0]),
        None => {
          if let Some(tally) = tally_of(value) {
            self.add(index, key, tally);
          }
        }
      }
    }
  }
}

/// A tally as the band a reading carries: its lowest, its highest and its mean to a hundredth.
fn band_of([min, max, sum, count]: Tally) -> Band {
  Band {
    min,
    max,
    mean: round_to(sum / count, 100.0),
  }
}

/// One reading of a machine out of its figures, each named `<piece>|<metric>` (docs/hardware.md,
/// 「History」); a figure whose name this layout does not hold is left out.
pub fn reading_of(machine: &str, at_ms: i64, span_ms: i64, figures: &BTreeMap<String, Band>) -> Reading {
  let mut reading = Reading {
    machine_name: machine.to_owned(),
    at: at_ms,
    span_ms,
    cpu: CpuFigures::default(),
    memory: MemoryFigures::default(),
    gpus: Vec::new(),
    storage: Vec::new(),
    fans: Vec::new(),
  };

  for (name, &band) in figures {
    let Some((piece, metric)) = name.rsplit_once('|') else {
      continue;
    };

    if piece == "cpu" {
      let slot = match metric {
        "use" => &mut reading.cpu.use_pct,
        "temp" => &mut reading.cpu.celsius,
        "watts" => &mut reading.cpu.watts,
        "mhz" => &mut reading.cpu.mhz,
        _ => continue,
      };

      *slot = Some(band);
    } else if piece == "memory" {
      if metric == "gb" {
        reading.memory.used_gb = Some(band);
      }
    } else if let Some(gpu) = piece.strip_prefix("gpu") {
      let (number, vram) = match gpu.strip_suffix(":vram") {
        Some(number) => (number, true),
        None => (gpu, false),
      };
      let Ok(index) = number.parse::<i64>() else {
        continue;
      };
      let at = match reading.gpus.iter().position(|one| one.index == index) {
        Some(at) => at,
        None => {
          reading.gpus.push(GpuFigures {
            index,
            use_pct: None,
            celsius: None,
            watts: None,
            mhz: None,
            vram_used_gb: None,
          });
          reading.gpus.len() - 1
        }
      };
      let figures = &mut reading.gpus[at];
      let slot = match (vram, metric) {
        (true, "gb") => &mut figures.vram_used_gb,
        (false, "use") => &mut figures.use_pct,
        (false, "temp") => &mut figures.celsius,
        (false, "watts") => &mut figures.watts,
        (false, "mhz") => &mut figures.mhz,
        _ => continue,
      };

      *slot = Some(band);
    } else if let Some(key) = piece.strip_prefix("storage:") {
      let at = match reading.storage.iter().position(|one| one.key == key) {
        Some(at) => at,
        None => {
          reading.storage.push(StorageFigures {
            key: key.to_owned(),
            used_gb: None,
            celsius: None,
          });
          reading.storage.len() - 1
        }
      };
      let figures = &mut reading.storage[at];

      match metric {
        "gb" => figures.used_gb = Some(band),
        "temp" => figures.celsius = Some(band),
        _ => {}
      }
    } else if let Some((chip, label)) = piece.strip_prefix("fan:").and_then(|fan| fan.split_once(':'))
      && metric == "rpm"
    {
      reading.fans.push(FanFigures {
        chip: chip.to_owned(),
        label: label.to_owned(),
        rpm: Some(band),
      });
    }
  }

  reading.gpus.sort_by_key(|gpu| gpu.index);
  reading
}

/// One live reading's figures as the reading of its second.
pub fn live_reading(machine: &str, at_seconds: i64, sample: &Map<String, Value>) -> Reading {
  let figures: BTreeMap<String, Band> = sample
    .iter()
    .filter_map(|(key, value)| {
      let value = value.as_f64().filter(|value| value.is_finite())?;

      Some((
        key.clone(),
        Band {
          min: value,
          max: value,
          mean: value,
        },
      ))
    })
    .collect();

  reading_of(machine, at_seconds * MS_PER_SECOND, DEFAULT_SPAN_MS, &figures)
}

/// Why a machine's readings could not be read.
#[derive(Debug)]
pub enum ReadingsError {
  /// The span is one no rows hold whole.
  Span(i64),
  Store(store::StoreError),
  Record(rusqlite::Error),
}

impl std::fmt::Display for ReadingsError {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Span(span_ms) => write!(
        out,
        "a span is whole seconds under a minute or whole minutes from a minute up, not {span_ms} ms"
      ),
      Self::Store(why) => write!(out, "{why}"),
      Self::Record(why) => write!(out, "the live record could not be read: {why}"),
    }
  }
}

/// Which of a machine's readings to read: the span, the first instant after which a stretch starts and the
/// instant before which it starts, both in milliseconds, and how many at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asked {
  pub span_ms: i64,
  pub after_ms: Option<i64>,
  pub before_ms: Option<i64>,
  pub limit: Option<usize>,
}

/// The machine's readings asked for at `now`, in seconds: the stretches of the span starting after `after`
/// and before `before`, no further back than their rows reach, oldest first, a stretch nothing was
/// recorded in left out, at most `limit` of them and whether more stand past the last.
pub fn read_readings(
  store: &Store,
  live: Option<&Reader>,
  machine: &str,
  asked: Asked,
  now: i64,
) -> Result<ReadingPage, ReadingsError> {
  let rows = Rows::of_span(asked.span_ms).ok_or(ReadingsError::Span(asked.span_ms))?;
  let span_ms = asked.span_ms;
  let span = span_ms / MS_PER_SECOND;
  // The first stretch starting after `after`, and none before the rows' reach.
  let reach = now - rows.reach_seconds();
  let from_reach = (reach + span - 1).div_euclid(span);
  let from_after = asked.after_ms.map_or(i64::MIN, |after| after.div_euclid(span_ms) + 1);
  let first = from_reach.max(from_after);
  // The last stretch starting before `before`.
  let before_ms = asked.before_ms.unwrap_or((now + 1) * MS_PER_SECOND);
  let last = (before_ms - 1).div_euclid(span_ms);

  if last < first {
    return Ok(ReadingPage::whole(Vec::new()));
  }

  let from = first * span;
  let until = (last + 1) * span;
  let mut stretches = Stretches::new((last - first + 1) as usize);
  let index_of = |instant: i64| (instant - from).div_euclid(span);

  match rows {
    Rows::Live => {
      if let Some(reader) = live {
        for sample in reader.samples(machine, from, until).map_err(ReadingsError::Record)? {
          stretches.add_sample(index_of(sample.at), &sample.sample);
        }
      }
    }
    Rows::Minutes => {
      for row in store.samples(machine, from, until).map_err(ReadingsError::Store)? {
        stretches.add_sample(index_of(row.at), &parsed(&row.figures));
      }
    }
    Rows::HalfHours => {
      for row in store.rollups(machine, from, until).map_err(ReadingsError::Store)? {
        if let Value::Object(figures) = serde_json::from_str::<Value>(&row.figures).unwrap_or(Value::Null) {
          for (key, value) in &figures {
            if let Some(tally) = tally_of(value) {
              stretches.add(index_of(row.at), key, tally);
            }
          }
        }
      }

      for row in store
        .samples_not_rolled_up(machine, from, until, ROLLUP_SECONDS)
        .map_err(ReadingsError::Store)?
      {
        stretches.add_sample(index_of(row.at), &parsed(&row.figures));
      }
    }
  }

  let limit = asked.limit.unwrap_or(usize::MAX);
  let recorded = stretches.by_index.len();
  let items: Vec<Reading> = stretches
    .by_index
    .into_iter()
    .take(limit)
    .map(|(index, tallies)| {
      let figures = tallies.into_iter().map(|(key, tally)| (key, band_of(tally))).collect();

      reading_of(machine, (from + index as i64 * span) * MS_PER_SECOND, span_ms, &figures)
    })
    .collect();

  Ok(ReadingPage {
    more: recorded > items.len(),
    items,
  })
}
