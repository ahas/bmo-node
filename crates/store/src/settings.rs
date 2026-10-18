//! The settings (docs/store.md, 「The tables」): a key and a text value each. A machine's live tracking and
//! its CPU limit are kept under the machine's name (docs/hardware.md, 「Tracking」 and 「Temperature
//! limits」), and the six machine limits under their own (docs/machine.md, 「The limits」).

use rusqlite::{OptionalExtension, params};

use crate::{Result, Store};

/// The settings key a machine's tracking is kept under; a machine with none is tracked.
pub fn tracking_key(machine: &str) -> String {
  format!("hardware_tracking:{machine}")
}

/// The settings key a machine's CPU limit is kept under.
pub fn cpu_limit_key(machine: &str) -> String {
  format!("hardware_cpu_limit:{machine}")
}

/// The prefix every machine limit's key opens with.
pub const LIMIT_PREFIX: &str = "machine_limit:";

const TRACKING_PREFIX: &str = "hardware_tracking:";

/// One of the machine's limits, with its default (docs/machine.md, 「The limits」).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineLimit {
  LaunchMemoryFullPct,
  LaunchMemoryAvailableGb,
  LaunchLoadPerThread,
  LaunchDiskFreeGb,
  AlertMemoryFullPct,
  AlertMinutes,
}

impl MachineLimit {
  pub const ALL: [Self; 6] = [
    Self::LaunchMemoryFullPct,
    Self::LaunchMemoryAvailableGb,
    Self::LaunchLoadPerThread,
    Self::LaunchDiskFreeGb,
    Self::AlertMemoryFullPct,
    Self::AlertMinutes,
  ];

  /// The limit's own name, which its key carries after the prefix.
  pub fn name(self) -> &'static str {
    match self {
      Self::LaunchMemoryFullPct => "launch_memory_full_pct",
      Self::LaunchMemoryAvailableGb => "launch_memory_available_gb",
      Self::LaunchLoadPerThread => "launch_load_per_thread",
      Self::LaunchDiskFreeGb => "launch_disk_free_gb",
      Self::AlertMemoryFullPct => "alert_memory_full_pct",
      Self::AlertMinutes => "alert_minutes",
    }
  }

  /// What the limit reads as where no positive number is set.
  pub fn default_value(self) -> f64 {
    match self {
      Self::LaunchMemoryFullPct => 5.0,
      Self::LaunchMemoryAvailableGb => 4.0,
      Self::LaunchLoadPerThread => 2.0,
      Self::LaunchDiskFreeGb => 20.0,
      Self::AlertMemoryFullPct => 10.0,
      Self::AlertMinutes => 2.0,
    }
  }

  /// The settings key the limit is kept under.
  pub fn key(self) -> String {
    format!("{LIMIT_PREFIX}{}", self.name())
  }
}

/// A number read the way a person's setting is read: the longest leading run that is one.
pub fn leading_float(text: &str) -> Option<f64> {
  let text = text.trim_start();

  (1..=text.len())
    .rev()
    .filter(|&end| text.is_char_boundary(end))
    .find_map(|end| text[..end].parse::<f64>().ok())
}

impl Store {
  /// A setting's value, or none where it is not set.
  pub fn setting(&self, key: &str) -> Result<Option<String>> {
    Ok(
      self
        .connection()
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
        .optional()?,
    )
  }

  /// A setting written.
  pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
    self.connection().execute(
      "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
      params![key, value],
    )?;

    Ok(())
  }

  /// A setting removed; whether it was set.
  pub fn remove_setting(&self, key: &str) -> Result<bool> {
    Ok(self.connection().execute("DELETE FROM settings WHERE key = ?1", [key])? > 0)
  }

  /// Whether a machine's live tracking is on: on unless it was turned off.
  pub fn tracking(&self, machine: &str) -> Result<bool> {
    Ok(self.setting(&tracking_key(machine))?.as_deref() != Some("off"))
  }

  /// A machine's live tracking switched.
  pub fn set_tracking(&self, machine: &str, on: bool) -> Result<()> {
    self.set_setting(&tracking_key(machine), if on { "on" } else { "off" })
  }

  /// Every machine whose tracking was turned off, by the name the monitor knows it by.
  pub fn untracked(&self) -> Result<Vec<String>> {
    let connection = self.connection();
    let mut query = connection
      .prepare("SELECT key FROM settings WHERE substr(key, 1, ?1) = ?2 AND value = 'off' ORDER BY key")?;
    let keys = query
      .query_map(params![TRACKING_PREFIX.len() as i64, TRACKING_PREFIX], |row| row.get::<_, String>(0))?
      .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(keys.into_iter().map(|key| key[TRACKING_PREFIX.len()..].to_owned()).collect())
  }

  /// The CPU limit set for a machine, in degrees, or none where none is set.
  pub fn cpu_limit(&self, machine: &str) -> Result<Option<f64>> {
    Ok(self.setting(&cpu_limit_key(machine))?.and_then(|value| value.parse::<f64>().ok()))
  }

  /// A machine's CPU limit kept as the text given, or cleared with none.
  pub fn set_cpu_limit(&self, machine: &str, celsius: Option<&str>) -> Result<()> {
    match celsius {
      Some(value) => self.set_setting(&cpu_limit_key(machine), value),
      None => self.remove_setting(&cpu_limit_key(machine)).map(drop),
    }
  }

  /// A machine limit as the settings hold it; a value that is not a positive number reads as the default.
  pub fn machine_limit(&self, limit: MachineLimit) -> Result<f64> {
    Ok(
      self
        .setting(&limit.key())?
        .and_then(|value| leading_float(&value))
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(limit.default_value()),
    )
  }
}
