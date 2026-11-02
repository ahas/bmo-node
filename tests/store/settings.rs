//! The settings: a machine's tracking and CPU limit, and the machine limits read as their defaults unless
//! set to a positive number.

use store::settings::{MachineLimit, leading_float};

use crate::store::scratch;

#[test]
fn a_machine_is_tracked_unless_its_tracking_was_turned_off() {
  let scratch = scratch();
  let store = &scratch.store;

  assert!(store.tracking("local").unwrap());
  store.set_tracking("lab", false).unwrap();
  store.set_tracking("box", false).unwrap();
  store.set_tracking("box", true).unwrap();

  assert!(!store.tracking("lab").unwrap());
  assert!(store.tracking("box").unwrap());
  assert_eq!(store.untracked().unwrap(), ["lab"]);
  assert_eq!(store.setting("hardware_tracking:lab").unwrap().as_deref(), Some("off"));
}

#[test]
fn a_cpu_limit_is_kept_per_machine_and_cleared_with_none() {
  let scratch = scratch();
  let store = &scratch.store;

  assert_eq!(store.cpu_limit("local").unwrap(), None);
  store.set_cpu_limit("local", Some("95")).unwrap();
  assert_eq!(store.cpu_limit("local").unwrap(), Some(95.0));
  assert_eq!(store.setting("hardware_cpu_limit:local").unwrap().as_deref(), Some("95"));
  store.set_cpu_limit("local", None).unwrap();
  assert_eq!(store.cpu_limit("local").unwrap(), None);
}

#[test]
fn a_limit_set_to_anything_but_a_positive_number_reads_as_the_default() {
  let scratch = scratch();
  let store = &scratch.store;

  for limit in MachineLimit::ALL {
    assert_eq!(store.machine_limit(limit).unwrap(), limit.default_value(), "{}", limit.name());
  }
  assert_eq!(MachineLimit::ALL.map(MachineLimit::default_value), [10.0, 2.0, 20.0]);

  store.set_setting("machine_limit:alert_disk_free_gb", "40 GB").unwrap();
  store.set_setting("machine_limit:alert_memory_full_pct", "-3").unwrap();
  store.set_setting("machine_limit:alert_minutes", "soon").unwrap();

  assert_eq!(store.machine_limit(MachineLimit::AlertDiskFreeGb).unwrap(), 40.0);
  assert_eq!(store.machine_limit(MachineLimit::AlertMemoryFullPct).unwrap(), 10.0);
  assert_eq!(store.machine_limit(MachineLimit::AlertMinutes).unwrap(), 2.0);
}

#[test]
fn a_setting_s_number_is_its_longest_leading_run_that_is_one() {
  assert_eq!(leading_float(" 12.5GB"), Some(12.5));
  assert_eq!(leading_float("3"), Some(3.0));
  assert_eq!(leading_float("GB"), None);
}

#[test]
fn a_setting_is_written_replaced_and_removed() {
  let scratch = scratch();
  let store = &scratch.store;

  store.set_setting("one", "a").unwrap();
  store.set_setting("one", "b").unwrap();
  assert_eq!(store.setting("one").unwrap().as_deref(), Some("b"));
  assert!(store.remove_setting("one").unwrap());
  assert!(!store.remove_setting("one").unwrap());
  assert_eq!(store.setting("one").unwrap(), None);
}
