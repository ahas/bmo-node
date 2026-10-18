//! The sensor library a machine's Windows is read through (docs/hardware.md, 「The Windows side」): where
//! it is kept, and the release and driver the setup puts there.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowsSensors {
  /// Where the library is kept, under %ProgramData%.
  pub library_dir: String,
  /// The release of LibreHardwareMonitor the setup puts there.
  pub release: String,
  /// The driver package the library reads the CPU and the board through.
  pub driver_package: String,
}

crate::part!(pub WINDOWS_SENSORS: WindowsSensors = "windows-sensors.toml");
