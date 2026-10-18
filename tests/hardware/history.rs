//! The history over node's store: a minute's readings tallied, the closed minutes folded once, each closed
//! half hour rolled up once and a bounded number at a time, and what grows old pruned.

use hardware::history::{self, ROLLUP_KEEP_SECONDS, ROLLUP_SECONDS, SAMPLE_KEEP_SECONDS, Tallies, parsed};
use hardware::record::Sample;
use serde_json::{Value, json};
use store::Store;

#[test]
fn a_minutes_readings_tally_each_figure_and_a_tally_adds_to_one() {
  let mut tallies = Tallies::default();

  tallies.add_sample(&parsed(r#"{"cpu|use":10,"cpu|temp":50.5}"#));
  tallies.add_sample(&parsed(r#"{"cpu|use":30.25,"gpu0|use":[1,5,6,2]}"#));
  tallies.add_sample(&parsed(r#"{"cpu|use":"high","gpu0|use":[2,9,9,0]}"#));

  let totals: Value = serde_json::from_str(&tallies.totals()).unwrap();

  assert_eq!(
    totals,
    json!({ "cpu|temp": [50.5, 50.5, 50.5, 1], "cpu|use": [10, 30.25, 40.25, 2], "gpu0|use": [1, 5, 6, 2] })
  );
}

fn sample(at: i64, usage: f64) -> Sample {
  Sample {
    at,
    sample: json!({ "cpu|use": usage }).as_object().unwrap().clone(),
  }
}

fn samples(store: &Store) -> Vec<(i64, Value)> {
  store
    .samples("box", i64::MIN, i64::MAX)
    .unwrap()
    .into_iter()
    .map(|row| (row.at, serde_json::from_str(&row.figures).unwrap()))
    .collect()
}

fn rollups(store: &Store) -> Vec<(i64, Value)> {
  store
    .rollups("box", i64::MIN, i64::MAX)
    .unwrap()
    .into_iter()
    .map(|row| (row.at, serde_json::from_str(&row.figures).unwrap()))
    .collect()
}

#[test]
fn the_history_folds_closed_minutes_rolls_up_closed_half_hours_once_and_keeps_each_its_time() {
  let store = Store::in_memory().unwrap();

  // The minute at 6000 already has its row; the one at 6180 is still open at 6200.
  store.put_samples("box", &[(6_000, r#"{"cpu|use":1}"#.into())]).unwrap();

  let record = [
    sample(6_010, 50.0),
    sample(6_061, 10.0),
    sample(6_062, 30.0),
    sample(6_150, 90.0),
    sample(6_190, 70.0),
  ];
  let folded = history::fold(&store, "box", 6_200, 3_600, |from, to| {
    record
      .iter()
      .filter(|one| one.at >= from && one.at < to)
      .map(|one| Sample {
        at: one.at,
        sample: one.sample.clone(),
      })
      .collect()
  })
  .unwrap();

  assert_eq!(folded, 2);
  assert_eq!(
    samples(&store),
    [
      (6_000, json!({ "cpu|use": 1 })),
      (6_060, json!({ "cpu|use": [10, 30, 40, 2] })),
      (6_120, json!({ "cpu|use": [90, 90, 90, 1] }))
    ]
  );
  assert_eq!(history::fold(&store, "box", 6_200, 3_600, |_, _| vec![]).unwrap(), 0);

  // Each closed half hour rolled up once, a bounded number at a time.
  let half = ROLLUP_SECONDS;
  let store = Store::in_memory().unwrap();

  store
    .put_samples(
      "box",
      &[
        (0, r#"{"cpu|use":10}"#.into()),
        (600, r#"{"cpu|use":30}"#.into()),
        (half, r#"{"cpu|use":50}"#.into()),
        (2 * half + 60, r#"{"cpu|use":70}"#.into()),
      ],
    )
    .unwrap();

  let now = 2 * half + 120;

  assert_eq!(history::roll_up(&store, now, 1).unwrap(), 1);
  assert_eq!(history::roll_up(&store, now, 200).unwrap(), 1);
  assert_eq!(history::roll_up(&store, now, 200).unwrap(), 0);
  assert_eq!(
    rollups(&store),
    [
      (0, json!({ "cpu|use": [10, 30, 40, 2] })),
      (half, json!({ "cpu|use": [50, 50, 50, 1] }))
    ]
  );

  // A minute's row two days once its half hour is rolled up, a rolled-up row a month.
  let now = ROLLUP_KEEP_SECONDS + 10 * half;
  let store = Store::in_memory().unwrap();

  store
    .put_samples(
      "box",
      &[
        (now - SAMPLE_KEEP_SECONDS - half, r#"{"cpu|use":2}"#.into()),
        (now - SAMPLE_KEEP_SECONDS - 3 * half, r#"{"cpu|use":3}"#.into()),
      ],
    )
    .unwrap();
  history::roll_up(&store, now - SAMPLE_KEEP_SECONDS - 2 * half, 200).unwrap();
  store.put_samples("box", &[(0, r#"{"cpu|use":1}"#.into())]).unwrap();

  assert_eq!(history::prune(&store, now).unwrap(), 2);
  assert_eq!(
    samples(&store).iter().map(|(at, _)| *at).collect::<Vec<_>>(),
    [now - SAMPLE_KEEP_SECONDS - half]
  );
  assert_eq!(
    rollups(&store).iter().map(|(at, _)| *at).collect::<Vec<_>>(),
    [now - SAMPLE_KEEP_SECONDS - 3 * half]
  );
}
