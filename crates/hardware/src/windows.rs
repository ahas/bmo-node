//! The Windows side of a machine (docs/hardware.md, 「The Windows side」, 「A Windows machine」): the
//! records the Windows scripts print, each a kind, a tab and its fields a tab apart, read into the
//! machine's own figures; and the command a login into Windows PowerShell is handed to run a script.

use std::io::Write;
use std::sync::LazyLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use regex::Regex;

use super::js::{parse_float, round, round_to, shown};
use super::probe::{Fan, Sensor, plausible_celsius};

#[derive(Clone, Debug, PartialEq)]
pub struct WindowsVolume {
  pub mount: String,
  pub fs_type: Option<String>,
  pub total_kb: f64,
  pub available_kb: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowsDisk {
  pub number: f64,
  pub model: String,
  pub media: Option<String>,
  pub bus: Option<String>,
  pub size_bytes: Option<f64>,
  pub celsius: Option<f64>,
  pub volumes: Vec<WindowsVolume>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowsCpu {
  pub model: String,
  pub cores: Option<f64>,
  pub logical: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowsThread {
  pub usage: Option<f64>,
  pub mhz: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowsReading {
  pub os: Option<String>,
  /// The kernel's own version, `10.0.22631`.
  pub version: Option<String>,
  pub cpu: Option<WindowsCpu>,
  /// Total and available memory, in kilobytes.
  pub memory: Option<(f64, f64)>,
  /// The page file, the swap of a Windows machine: total and free, in kilobytes.
  pub swap: Option<(f64, f64)>,
  pub uptime_seconds: Option<f64>,
  pub cpu_usage: Option<f64>,
  /// By logical processor, in Windows' own order.
  pub threads: Vec<WindowsThread>,
  pub cpu_watts: Option<f64>,
  pub sensors: Vec<Sensor>,
  pub fans: Vec<Fan>,
  pub disks: Vec<WindowsDisk>,
  /// Whether the sensor library was there and opened; none where the script never got that far.
  pub library: Option<&'static str>,
}

/// A field as Windows prints a number, a decimal comma read as a point.
fn num(text: Option<&str>) -> Option<f64> {
  let text = text?.trim();

  if text.is_empty() {
    return None;
  }

  parse_float(&text.replacen(',', ".", 1))
}

/// A field as text, where it holds any.
fn text(cells: &[&str], at: usize) -> Option<String> {
  cells.get(at).map(|cell| cell.trim()).filter(|cell| !cell.is_empty()).map(str::to_owned)
}

/// A counter's percentage as a share, 0 to 1, to a tenth of a percent: the utility counter runs past a
/// hundred while a core boosts.
fn fraction(percent: Option<f64>) -> Option<f64> {
  percent.map(|percent| round(percent.clamp(0.0, 100.0) * 10.0) / 1000.0)
}

static TCTL: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^Core \(Tctl(/Tdie)?\)$").expect("the Tctl pattern"));
static CCD: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^CCD ?(\d+) \(Tdie\)$").expect("the CCD pattern"));
static CORE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(?:CPU )?Core #(\d+)$").expect("the core pattern"));
static DISK_SOURCE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^disk(\d+)$").expect("the disk pattern"));
static NOT_A_READING: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)warning|critical|limit").expect("the threshold pattern"));
static DISK_READING: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(Composite )?Temperature$").expect("the disk reading pattern"));
static MICROSOFT: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^Microsoft\s+").expect("the edition pattern"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("the spaces pattern"));

/// LibreHardwareMonitor's CPU sensor names, said the way the kernel's own drivers name them. A figure
/// the library works out from others is left out.
fn cpu_sensor(label: &str, celsius: f64) -> Option<Sensor> {
  let sensor = |chip: &str, label: String| Sensor {
    chip: chip.to_owned(),
    device: None,
    label,
    celsius: round_to(celsius, 10.0),
    limit_celsius: None,
  };

  if TCTL.is_match(label) {
    return Some(sensor("k10temp", "Tctl".into()));
  }

  if let Some(ccd) = CCD.captures(label) {
    return Some(sensor("k10temp", format!("Tccd{}", &ccd[1])));
  }

  if label == "CPU Package" {
    return Some(sensor("coretemp", "Package id 0".into()));
  }

  let core = CORE.captures(label)?;
  let number: f64 = core[1].parse().ok()?;

  Some(sensor("coretemp", format!("Core {}", shown(number - 1.0))))
}

/// The board chip's own short name, as the kernel would call it: `Nuvoton NCT6779D` is `nct6779d`.
fn board_chip(name: &str) -> String {
  SPACES.split(name).last().unwrap_or(name).to_lowercase()
}

/// A disk's own temperature: the drive's composite where it reports several, never a threshold.
fn disk_temperature(readings: &[(String, f64)]) -> Option<f64> {
  let usable: Vec<&(String, f64)> =
    readings.iter().filter(|(label, _)| !NOT_A_READING.is_match(label)).collect();
  let chosen = usable.iter().find(|(label, _)| DISK_READING.is_match(label)).or(usable.first())?;

  Some(round_to(chosen.1, 10.0))
}

/// The records as figures; none where the script printed nothing.
pub fn read_windows(lines: &[String]) -> Option<WindowsReading> {
  if lines.is_empty() {
    return None;
  }

  let mut found = WindowsReading::default();
  let mut threads: Vec<Option<WindowsThread>> = Vec::new();
  let mut disks: Vec<WindowsDisk> = Vec::new();
  let mut disk_temps: Vec<(f64, Vec<(String, f64)>)> = Vec::new();
  let mut meter_watts = None;
  let mut library_watts = None;

  for line in lines {
    let mut parts = line.split('\t');
    let kind = parts.next().unwrap_or_default();
    let cells: Vec<&str> = parts.collect();
    let cell = |at: usize| cells.get(at).copied();

    match kind {
      "os" if cells.first().is_some_and(|caption| !caption.is_empty()) => {
        let caption = MICROSOFT.replace(cells[0].trim(), "").into_owned();

        found.os = Some(match cell(1).filter(|build| !build.is_empty()) {
          Some(build) => format!("{caption} (build {})", build.trim()),
          None => caption,
        });
        found.version = text(&cells, 2);
      }
      "model" if text(&cells, 0).is_some() => {
        found.cpu = Some(WindowsCpu {
          model: SPACES.replace_all(cells[0].trim(), " ").into_owned(),
          cores: num(cell(1)),
          logical: num(cell(2)),
        });
      }
      "swap" => {
        if let (Some(total), Some(used)) = (num(cell(0)), num(cell(1))) {
          found.swap = Some((total * 1024.0, (total - used).max(0.0) * 1024.0));
        }
      }
      "mem" => {
        if let (Some(total), Some(free)) = (num(cell(0)), num(cell(1))) {
          found.memory = Some((total, free));
        }
      }
      "uptime" => found.uptime_seconds = num(cell(0)),
      "total" => found.cpu_usage = fraction(num(cell(0))),
      "power" => meter_watts = num(cell(0)),
      "cpupower" => library_watts = num(cell(0)),
      "cpu" => {
        let Some(index) = num(cell(0)).filter(|index| *index >= 0.0 && index.fract() == 0.0) else {
          continue;
        };
        let index = index as usize;
        let mhz = num(cell(2)).filter(|mhz| *mhz > 0.0).map(round);

        if threads.len() <= index {
          threads.resize(index + 1, None);
        }

        threads[index] = Some(WindowsThread {
          usage: fraction(num(cell(1))),
          mhz,
        });
      }
      "disk" => {
        let Some(number) = num(cell(0)) else {
          continue;
        };
        let media = text(&cells, 2).filter(|media| media != "Unspecified");
        let disk = WindowsDisk {
          number,
          model: text(&cells, 1).unwrap_or_else(|| format!("Disk {}", shown(number))),
          media,
          bus: text(&cells, 3),
          size_bytes: num(cell(4)),
          celsius: None,
          volumes: vec![],
        };

        match disks.iter().position(|one| one.number == number) {
          Some(at) => disks[at] = disk,
          None => disks.push(disk),
        }
      }
      "vol" => {
        let number = num(cell(0)).unwrap_or(-1.0);

        if let (Some(disk), Some(total), Some(free)) =
          (disks.iter_mut().find(|one| one.number == number), num(cell(3)), num(cell(4)))
        {
          disk.volumes.push(WindowsVolume {
            mount: cell(1).unwrap_or_default().trim().to_owned(),
            fs_type: text(&cells, 2),
            total_kb: round(total / 1024.0),
            available_kb: round(free / 1024.0),
          });
        }
      }
      "lhm" => {
        found.library = Some(match cell(0) {
          Some("ok") => "ok",
          Some("missing") => "missing",
          _ => "failed",
        });
      }
      "temp" | "fan" => {
        let source = cell(0).unwrap_or_default();
        let label = cell(1).unwrap_or_default();
        let Some(value) = num(cell(2)) else {
          continue;
        };

        if let Some(disk) = DISK_SOURCE.captures(source).filter(|_| kind == "temp") {
          let number: f64 = disk[1].parse().unwrap_or(-1.0);

          match disk_temps.iter_mut().find(|(one, _)| *one == number) {
            Some((_, list)) => list.push((label.to_owned(), value)),
            None => disk_temps.push((number, vec![(label.to_owned(), value)])),
          }
        } else if source.starts_with("Cpu:") && kind == "temp" {
          if let Some(sensor) = cpu_sensor(label, value).filter(|_| value > 0.0) {
            found.sensors.push(sensor);
          }
        } else if source.starts_with("SuperIO:") || source.starts_with("Motherboard:") {
          let chip = board_chip(&source[source.find(':').map_or(0, |at| at + 1)..]);

          if kind == "temp" {
            if plausible_celsius(value) {
              found.sensors.push(Sensor {
                chip,
                device: None,
                label: label.to_owned(),
                celsius: round_to(value, 10.0),
                limit_celsius: None,
              });
            }
          } else {
            found.fans.push(Fan {
              chip,
              label: label.to_owned(),
              rpm: round(value),
            });
          }
        }
      }
      _ => {}
    }
  }

  for (number, readings) in &disk_temps {
    if let Some(disk) = disks.iter_mut().find(|one| one.number == *number) {
      disk.celsius = disk_temperature(readings);
    }
  }

  found.cpu_watts = match (meter_watts, library_watts) {
    (Some(meter), _) if meter > 0.0 => Some(round_to(meter, 10.0)),
    (_, Some(library)) if library > 0.0 => Some(round_to(library, 10.0)),
    _ => None,
  };
  disks.sort_by(|a, b| a.number.total_cmp(&b.number));
  found.disks = disks;
  found.threads = threads.into_iter().map(Option::unwrap_or_default).collect();

  Some(found)
}

/// A script as Windows PowerShell takes one encoded command: its text in UTF-16, as base64.
fn encoded(script: &str) -> String {
  let units: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();

  STANDARD.encode(units)
}

/// The command a login into Windows PowerShell is handed to run a script: Windows PowerShell started on
/// an encoded command that unpacks the script and runs it. The script travels compressed, so a command
/// line stays well inside what Windows takes whatever the script's length, and nothing is written on the
/// machine.
pub fn powershell_command(script: &str) -> String {
  let mut packer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
  let packed = packer
    .write_all(script.as_bytes())
    .and_then(|()| packer.finish())
    .map(|bytes| STANDARD.encode(bytes))
    .unwrap_or_default();
  let unpack = format!(
    "$z = New-Object IO.Compression.GZipStream((New-Object IO.MemoryStream(,[Convert]::FromBase64String('{packed}'))), [IO.Compression.CompressionMode]::Decompress)\n& ([ScriptBlock]::Create((New-Object IO.StreamReader($z, [Text.Encoding]::UTF8)).ReadToEnd()))"
  );

  format!("powershell -NoProfile -NonInteractive -EncodedCommand {}", encoded(&unpack))
}
