//! Every machine's hardware kept for a month (docs/hardware.md, 「History」). Each closed minute of the
//! live record becomes one row per machine of each figure's lowest, highest, sum and count, each figure
//! named by the piece of hardware it belongs to and what it measures; every closed half hour is rolled
//! up the same way into one row, which is what a week or a month is drawn out of. The rows stand in node's
//! store.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use store::Store;
use store::history::Row;

use super::js::round_to;
use super::record::Sample;

/// The stretch one minute's row covers, and one rolled-up row.
pub const MINUTE_SECONDS: i64 = 60;
pub const ROLLUP_SECONDS: i64 = 30 * 60;

/// How long a minute's row is kept once its half hour is rolled up, and how long a rolled-up row is
/// kept: past the longest span a screen asks for, the oldest going first.
pub const SAMPLE_KEEP_SECONDS: i64 = 2 * 86_400;
pub const ROLLUP_KEEP_SECONDS: i64 = 31 * 86_400;

/// How many half hours one pass rolls up at most, so a month with none is taken in over several.
pub const ROLLUP_BATCH: i64 = 200;

/// A figure over a stretch: its lowest, its highest, the sum and how many readings.
type Tally = [f64; 4];

fn tally_of(value: &Value) -> Option<Tally> {
  let values = value.as_array().filter(|values| values.len() == 4)?;
  let read: Vec<f64> = values.iter().map(Value::as_f64).collect::<Option<_>>()?;

  Some([read[0], read[1], read[2], read[3]])
}

/// A figure as JSON, without a point where it is whole.
fn figure(value: f64) -> Value {
  if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
    json!(value as i64)
  } else {
    json!(value)
  }
}

/// One stretch's tallies, every reading or rolled-up row adding to them.
#[derive(Default)]
pub struct Tallies(BTreeMap<String, Tally>);

impl Tallies {
  fn add(&mut self, key: &str, [min, max, sum, count]: Tally) {
    if count <= 0.0 {
      return;
    }

    let known = self.0.entry(key.to_owned()).or_insert([min, max, 0.0, 0.0]);

    *known = [known[0].min(min), known[1].max(max), known[2] + sum, known[3] + count];
  }

  /// A row's figures, each a single reading or a tally of several.
  pub fn add_sample(&mut self, sample: &Map<String, Value>) {
    for (key, value) in sample {
      match value.as_f64().filter(|value| value.is_finite()) {
        Some(reading) => self.add(key, [reading, reading, reading, 1.0]),
        None => {
          if let Some(tally) = tally_of(value) {
            self.add(key, tally);
          }
        }
      }
    }
  }

  /// One tally per figure, its sum to a hundredth, as JSON.
  pub fn totals(&self) -> String {
    let found: Map<String, Value> = self
      .0
      .iter()
      .map(|(key, [min, max, sum, count])| {
        (
          key.clone(),
          Value::Array(vec![figure(*min), figure(*max), figure(round_to(*sum, 100.0)), figure(*count)]),
        )
      })
      .collect();

    Value::Object(found).to_string()
  }
}

/// A row's figures read out of its JSON text, none where it does not read.
pub fn parsed(text: &str) -> Map<String, Value> {
  serde_json::from_str(text).unwrap_or_default()
}

/// Every minute closed by `now` since the machine's last row, as far back as the live record reaches,
/// folded out of the record's readings into one row each under the minute's start; a minute with no
/// reading leaves no row. How many rows were written.
pub fn fold(
  store: &Store,
  machine: &str,
  now: i64,
  reach: i64,
  samples: impl FnOnce(i64, i64) -> Vec<Sample>,
) -> store::Result<usize> {
  let last = store.last_sample_at(machine)?;
  let closed = now.div_euclid(MINUTE_SECONDS) * MINUTE_SECONDS;
  let after_last = last.map_or(0, |last| last.div_euclid(MINUTE_SECONDS) * MINUTE_SECONDS + MINUTE_SECONDS);
  let from = after_last.max((now - reach).div_euclid(MINUTE_SECONDS) * MINUTE_SECONDS);

  if from >= closed {
    return Ok(0);
  }

  let mut minutes: BTreeMap<i64, Tallies> = BTreeMap::new();

  for one in samples(from, closed) {
    minutes
      .entry(one.at.div_euclid(MINUTE_SECONDS) * MINUTE_SECONDS)
      .or_default()
      .add_sample(&one.sample);
  }

  let rows: Vec<(i64, String)> =
    minutes.iter().map(|(minute, tallies)| (*minute, tallies.totals())).collect();

  store.put_samples(machine, &rows)?;

  Ok(rows.len())
}

/// The half hours that have closed by `now` and have no rolled-up row yet, the oldest first and at most
/// `limit` of them, each rolled up out of the minutes' rows; how many were.
pub fn roll_up(store: &Store, now: i64, limit: i64) -> store::Result<usize> {
  let closed = now.div_euclid(ROLLUP_SECONDS) * ROLLUP_SECONDS;
  let due = store.rollups_due(closed, ROLLUP_SECONDS, limit)?;
  let mut rows = Vec::with_capacity(due.len());

  for (machine, grain) in due {
    let mut tallies = Tallies::default();

    for minute in store.samples(&machine, grain, grain + ROLLUP_SECONDS)? {
      tallies.add_sample(&parsed(&minute.figures));
    }

    rows.push(Row {
      machine,
      at: grain,
      figures: tallies.totals(),
    });
  }

  store.put_rollups(&rows)?;

  Ok(rows.len())
}

/// Rolled-up rows past the month kept, and minutes' rows past their two days whose half hour is rolled
/// up — or past the month whatever became of it — removed; how many went.
pub fn prune(store: &Store, now: i64) -> store::Result<usize> {
  store.prune_history(now - ROLLUP_KEEP_SECONDS, now - SAMPLE_KEEP_SECONDS, ROLLUP_SECONDS)
}
