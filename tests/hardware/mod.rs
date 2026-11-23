//! The hardware monitor's tests: numbers read the console's way, the probe's outputs read as the fixtures
//! hold them, the Windows command, the roads and the hub's exec stream, the live runs, the live record, the
//! history over node's store, the recorder keeping its runs to the tracked machines, and the Windows live
//! scripts run with their queries stood in for.

mod chips;
mod history;
mod hub_exec;
mod js;
mod live;
mod memory;
mod parity;
mod probe;
mod recorder;
mod roads;
mod series;
mod storage;
mod view;
mod windows;
mod windows_live;

use std::path::PathBuf;

/// The probe's outputs and the readings written beside them.
pub fn fixtures() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/hardware/fixtures")
}
