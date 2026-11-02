//! The hardware history's rows: minute rows written and read, the half hours due a rollup, rollups
//! written, and what grows old pruned.

use store::history::Row;

use crate::store::scratch;

const HALF_HOUR: i64 = 1_800;

fn row(machine: &str, at: i64, figures: &str) -> Row {
  Row {
    machine: machine.into(),
    at,
    figures: figures.into(),
  }
}

#[test]
fn minute_rows_are_written_replaced_and_read_in_their_order() {
  let scratch = scratch();
  let store = &scratch.store;

  assert_eq!(store.last_sample_at("local").unwrap(), None);
  store
    .put_samples("local", &[(120, "{\"a\":1}".into()), (60, "{\"a\":0}".into())])
    .unwrap();
  store.put_samples("local", &[(120, "{\"a\":2}".into())]).unwrap();
  store.put_samples("lab", &[(60, "{}".into())]).unwrap();

  assert_eq!(store.last_sample_at("local").unwrap(), Some(120));
  assert_eq!(
    store.samples("local", 0, 1_000).unwrap(),
    [row("local", 60, "{\"a\":0}"), row("local", 120, "{\"a\":2}")]
  );
  assert_eq!(store.samples("local", 61, 120).unwrap(), []);
}

#[test]
fn a_closed_half_hour_is_due_until_it_is_rolled_up() {
  let scratch = scratch();
  let store = &scratch.store;

  store
    .put_samples(
      "local",
      &[(60, "{}".into()), (HALF_HOUR + 60, "{}".into()), (2 * HALF_HOUR + 60, "{}".into())],
    )
    .unwrap();
  store.put_samples("lab", &[(120, "{}".into())]).unwrap();

  let closed = 2 * HALF_HOUR;

  assert_eq!(
    store.rollups_due(closed, HALF_HOUR, 10).unwrap(),
    [("lab".to_owned(), 0), ("local".to_owned(), 0), ("local".to_owned(), HALF_HOUR)]
  );
  assert_eq!(store.rollups_due(closed, HALF_HOUR, 1).unwrap().len(), 1);

  store.put_rollups(&[row("local", 0, "{\"t\":1}"), row("lab", 0, "{}")]).unwrap();

  assert_eq!(store.rollups_due(closed, HALF_HOUR, 10).unwrap(), [("local".to_owned(), HALF_HOUR)]);
  assert_eq!(store.rollups("local", 0, HALF_HOUR).unwrap(), [row("local", 0, "{\"t\":1}")]);
}

#[test]
fn old_rollups_go_and_minute_rows_go_once_rolled_up_or_past_the_month() {
  let scratch = scratch();
  let store = &scratch.store;

  store
    .put_samples(
      "local",
      &[(60, "{}".into()), (HALF_HOUR + 60, "{}".into()), (10 * HALF_HOUR, "{}".into())],
    )
    .unwrap();
  store.put_rollups(&[row("local", 0, "{}"), row("local", 10 * HALF_HOUR, "{}")]).unwrap();

  // Rollups before the second half hour go; minute rows before the twentieth half hour go where rolled up.
  let removed = store.prune_history(HALF_HOUR, 20 * HALF_HOUR, HALF_HOUR).unwrap();

  assert_eq!(removed, 1 + 2);
  assert_eq!(
    store.samples("local", 0, 100 * HALF_HOUR).unwrap(),
    [row("local", HALF_HOUR + 60, "{}")]
  );
  assert_eq!(
    store.rollups("local", 0, 100 * HALF_HOUR).unwrap(),
    [row("local", 10 * HALF_HOUR, "{}")]
  );
}
