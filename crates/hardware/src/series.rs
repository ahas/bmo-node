//! What a screen is handed of a machine's recorded figures (docs/hardware.md, 「History」 and 「Live」):
//! the live record's readings as columns, and one span of the history ending now, cut into points of a
//! fixed width, each its lowest, highest and mean.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use store::Store;

use super::history::{ROLLUP_SECONDS, parsed};
use super::js::round_to;
use super::record::Reader;
pub use bmo_node_client::hardware::{HistoryBand, HistoryRange, LiveRange};

/// The finest the store's rows are: one a minute. A narrower point is the live record's.
pub const HISTORY_ROW_SECONDS: i64 = 60;

/// The span a screen opens on.
pub const HISTORY_FIRST_SPAN: &str = "1h";

/// The spans a screen may ask for, shortest first: each name, its length and the width of one of its
/// points, some sixty or more points to a span.
pub const HISTORY_SPANS: [(&str, i64, i64); 7] = [
  ("5m", 5 * 60, 5),
  ("15m", 15 * 60, 15),
  ("1h", 3_600, 60),
  ("6h", 6 * 3_600, 5 * 60),
  ("24h", 86_400, 5 * 60),
  ("7d", 7 * 86_400, 30 * 60),
  ("30d", 30 * 86_400, 2 * 3_600),
];

/// A span's length and point width by its name, none for a name the monitor does not draw.
pub fn span(name: &str) -> Option<(i64, i64)> {
  HISTORY_SPANS
    .iter()
    .find(|(one, _, _)| *one == name)
    .map(|(_, seconds, point)| (*seconds, *point))
}

/// Whether a figure measures one of the given things: the part of its name after its last `|`.
pub fn measures(key: &str, metrics: &[String]) -> bool {
  let metric = key.rsplit('|').next().unwrap_or(key);

  metrics.iter().any(|one| one == metric)
}

/// A figure over a stretch: its lowest, its highest, the sum and how many readings.
type Tally = [f64; 4];

fn tally_of(value: &Value) -> Option<Tally> {
  let values = value.as_array().filter(|values| values.len() == 4)?;
  let read: Vec<f64> = values.iter().map(Value::as_f64).collect::<Option<_>>()?;

  Some([read[0], read[1], read[2], read[3]])
}

/// Tallies gathered into points: every reading or rolled-up row adds to the point its instant falls in.
struct Points {
  points: usize,
  by_key: BTreeMap<String, Vec<Option<Tally>>>,
}

impl Points {
  fn new(points: usize) -> Self {
    Self {
      points,
      by_key: BTreeMap::new(),
    }
  }

  fn add(&mut self, index: i64, key: &str, [min, max, sum, count]: Tally) {
    if index < 0 || index as usize >= self.points || count <= 0.0 {
      return;
    }

    let row = self.by_key.entry(key.to_owned()).or_insert_with(|| vec![None; self.points]);
    let slot = &mut row[index as usize];

    *slot = Some(match *slot {
      None => [min, max, sum, count],
      Some(known) => [known[0].min(min), known[1].max(max), known[2] + sum, known[3] + count],
    });
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

  fn bands(self) -> BTreeMap<String, HistoryBand> {
    self
      .by_key
      .into_iter()
      .map(|(key, row)| {
        let band = HistoryBand {
          min: row.iter().map(|tally| tally.map(|one| one[0])).collect(),
          max: row.iter().map(|tally| tally.map(|one| one[1])).collect(),
          mean: row.iter().map(|tally| tally.map(|one| round_to(one[2] / one[3], 100.0))).collect(),
        };

        (key, band)
      })
      .collect()
  }
}

/// Milliseconds in a second: a range answers its instants in epoch milliseconds, and the record and the
/// store keep them in seconds.
pub const MS_PER_SECOND: i64 = 1_000;

/// Instants in seconds, as a range answers them.
fn in_ms(at: Vec<i64>) -> Vec<i64> {
  at.into_iter().map(|second| second * MS_PER_SECOND).collect()
}

/// The starts of the points between two instants, the first at a whole point.
fn starts(from: i64, to: i64, point_seconds: i64) -> Vec<i64> {
  let start = from.div_euclid(point_seconds) * point_seconds;

  (start..to).step_by(point_seconds as usize).collect()
}

/// The store's rows of a machine between two instants, gathered into points of a fixed width: a point
/// as wide as a half hour or a whole number of them out of the rolled-up rows, with the minutes' rows of
/// any half hour not rolled up yet; any other out of the minutes' rows.
pub fn read_history(
  store: &Store,
  machine: &str,
  from: i64,
  to: i64,
  point_seconds: i64,
) -> store::Result<HistoryRange> {
  let at = starts(from, to, point_seconds);
  let start = at.first().copied().unwrap_or(from);
  let mut points = Points::new(at.len());
  let index_of = |instant: i64| (instant - start).div_euclid(point_seconds);
  let rolled = point_seconds % ROLLUP_SECONDS == 0;

  if rolled {
    for row in store.rollups(machine, start, to)? {
      if let Value::Object(figures) = serde_json::from_str::<Value>(&row.figures).unwrap_or(Value::Null) {
        for (key, value) in &figures {
          if let Some(tally) = tally_of(value) {
            points.add(index_of(row.at), key, tally);
          }
        }
      }
    }
  }

  let minutes = if rolled {
    store.samples_not_rolled_up(machine, start, to, ROLLUP_SECONDS)?
  } else {
    store.samples(machine, start, to)?
  };

  for row in minutes {
    points.add_sample(index_of(row.at), &parsed(&row.figures));
  }

  Ok(HistoryRange {
    at: in_ms(at),
    point_seconds,
    series: points.bands(),
  })
}

/// The live record's readings between two instants, gathered into points of a fixed width as the store's
/// rows are; a record not there reads as an empty range.
pub fn read_live_history(
  live: Option<&Reader>,
  machine: &str,
  from: i64,
  to: i64,
  point_seconds: i64,
) -> rusqlite::Result<HistoryRange> {
  let at = starts(from, to, point_seconds);
  let start = at.first().copied().unwrap_or(from);
  let mut points = Points::new(at.len());
  let range = match live {
    Some(reader) => reader.range(machine, start, &[])?,
    None => LiveRange::default(),
  };

  add_live(&mut points, &range, start, to, point_seconds);

  Ok(HistoryRange {
    at: in_ms(at),
    point_seconds,
    series: points.bands(),
  })
}

/// A live range's readings before `to` added to the points they fall in.
fn add_live(points: &mut Points, range: &LiveRange, start: i64, to: i64, point_seconds: i64) {
  for (row, instant) in range.at.iter().enumerate() {
    let instant = instant.div_euclid(MS_PER_SECOND);

    if instant >= to {
      continue;
    }

    let sample: Map<String, Value> = range
      .series
      .iter()
      .filter_map(|(key, column)| column[row].map(|value| (key.clone(), Value::from(value))))
      .collect();

    points.add_sample((instant - start).div_euclid(point_seconds), &sample);
  }
}

/// A live range gathered into points of a fixed width between two instants.
pub fn gather_live(range: &LiveRange, from: i64, to: i64, point_seconds: i64) -> HistoryRange {
  let at = starts(from, to, point_seconds);
  let start = at.first().copied().unwrap_or(from);
  let mut points = Points::new(at.len());

  add_live(&mut points, range, start, to, point_seconds);

  HistoryRange {
    at: in_ms(at),
    point_seconds,
    series: points.bands(),
  }
}

/// Why a span could not be read.
#[derive(Debug)]
pub enum SpanError {
  Store(store::StoreError),
  Record(rusqlite::Error),
}

impl std::fmt::Display for SpanError {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Store(why) => write!(out, "{why}"),
      Self::Record(why) => write!(out, "the live record could not be read: {why}"),
    }
  }
}

/// One of the spans a screen asks for, ending now: out of the live record where its points are narrower
/// than the store's rows, out of the store otherwise.
pub fn read_span(
  store: &Store,
  live: Option<&Reader>,
  machine: &str,
  seconds: i64,
  point_seconds: i64,
  now: i64,
) -> Result<HistoryRange, SpanError> {
  if point_seconds < HISTORY_ROW_SECONDS {
    read_live_history(live, machine, now - seconds, now, point_seconds).map_err(SpanError::Record)
  } else {
    read_history(store, machine, now - seconds, now, point_seconds).map_err(SpanError::Store)
  }
}
