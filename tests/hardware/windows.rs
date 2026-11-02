//! The Windows side of a machine: the records the Windows scripts print read into the machine's own
//! figures, and the command a login into Windows PowerShell is handed to run a script.

use std::io::Read;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hardware::probe::{Fan, Sensor};
use hardware::windows::{WindowsThread, powershell_command, read_windows};

fn lines(records: &[&str]) -> Vec<String> {
  records.iter().map(|one| (*one).to_owned()).collect()
}

fn sensor(chip: &str, label: &str, celsius: f64) -> Sensor {
  Sensor {
    chip: chip.into(),
    device: None,
    label: label.into(),
    celsius,
    limit_celsius: None,
  }
}

const INTEL: &[&str] = &[
  "os\tMicrosoft Windows 11 Enterprise\t26100",
  "mem\t67015860\t44764512",
  "uptime\t530153",
  "total\t36.2",
  "cpu\t0\t41.7\t3600",
  "cpu\t1\t\t0",
  "disk\t0\tST3000DM007-1WY10G\tHDD\tSATA\t3000592982016",
  "disk\t1\tSamsung SSD 970 EVO 1TB\tSSD\tNVMe\t1000204886016",
  "disk\t4\tSeagate Expansion Desk\tUnspecified\tUSB\t8001563221504",
  "vol\t1\tC:\tNTFS\t999156609024\t104610181120",
  "lhm\tok",
  "temp\tSuperIO:Nuvoton NCT6798D\tTemperature #1\t39",
  "temp\tSuperIO:Nuvoton NCT6798D\tTemperature #7\t-62",
  "fan\tSuperIO:Nuvoton NCT6798D\tFan #2\t641.02",
  "cpupower\t35.9",
  "temp\tCpu:Intel Core i9-9900KF\tCore Max\t48",
  "temp\tCpu:Intel Core i9-9900KF\tCPU Core #1\t43",
  "temp\tCpu:Intel Core i9-9900KF\tCPU Core #1 Distance to TjMax\t57",
  "temp\tCpu:Intel Core i9-9900KF\tCPU Package\t48",
  "temp\tdisk1\tComposite Temperature\t32",
  "temp\tdisk1\tTemperature #2\t41.85",
  "temp\tdisk1\tWarning Temperature\t67",
];

#[test]
fn reads_the_machine_s_own_figures_and_names_its_cpu_sensors_as_the_kernel_s_drivers_do() {
  let found = read_windows(&lines(INTEL)).unwrap();

  assert_eq!(found.os.as_deref(), Some("Windows 11 Enterprise (build 26100)"));
  assert_eq!(found.memory, Some((67_015_860.0, 44_764_512.0)));
  assert_eq!(found.uptime_seconds, Some(530_153.0));
  assert_eq!(found.cpu_usage, Some(0.362));
  assert_eq!(found.library, Some("ok"));
  // No Energy Meter reading: the library's own package power stands in.
  assert_eq!(found.cpu_watts, Some(35.9));
  assert_eq!(
    found.threads,
    [
      WindowsThread {
        usage: Some(0.417),
        mhz: Some(3600.0)
      },
      WindowsThread {
        usage: None,
        mhz: None
      }
    ]
  );
  assert_eq!(
    found.sensors,
    [
      sensor("nct6798d", "Temperature #1", 39.0),
      sensor("coretemp", "Core 0", 43.0),
      sensor("coretemp", "Package id 0", 48.0)
    ]
  );
  assert_eq!(
    found.fans,
    [Fan {
      chip: "nct6798d".into(),
      label: "Fan #2".into(),
      rpm: 641.0
    }]
  );
  assert_eq!(
    found
      .disks
      .iter()
      .map(|one| (one.number, one.media.clone(), one.bus.clone(), one.celsius, one.volumes.len()))
      .collect::<Vec<_>>(),
    [
      (0.0, Some("HDD".to_owned()), Some("SATA".to_owned()), None, 0),
      (1.0, Some("SSD".to_owned()), Some("NVMe".to_owned()), Some(32.0), 1),
      (4.0, None, Some("USB".to_owned()), None, 0)
    ]
  );
}

#[test]
fn amd_s_control_and_die_temperatures_take_k10temp_s_names_and_the_energy_meter_s_power_comes_first() {
  let found = read_windows(&lines(&[
    "power\t24.33",
    "cpupower\t57.3",
    "temp\tCpu:AMD Ryzen 7 5800X\tCore (Tctl/Tdie)\t62.875",
    "temp\tCpu:AMD Ryzen 7 5800X\tCCD1 (Tdie)\t58",
  ]))
  .unwrap();

  assert_eq!(found.cpu_watts, Some(24.3));
  assert_eq!(found.sensors, [sensor("k10temp", "Tctl", 62.9), sensor("k10temp", "Tccd1", 58.0)]);
}

#[test]
fn a_machine_without_the_library_says_so_and_a_machine_that_printed_nothing_has_no_windows_side() {
  assert_eq!(read_windows(&lines(&["lhm\tmissing"])).unwrap().library, Some("missing"));
  assert!(read_windows(&[]).is_none());
}

#[test]
fn a_windows_machine_s_own_records_give_its_kernel_version_its_cpu_s_counts_and_its_page_file() {
  let found = read_windows(&lines(&[
    "os\tMicrosoft Windows 11 Pro\t22631\t10.0.22631",
    "model\tAMD Ryzen 7 5800X 8-Core Processor   \t8\t16",
    "swap\t4096\t512",
  ]))
  .unwrap();
  let cpu = found.cpu.unwrap();

  assert_eq!(found.os.as_deref(), Some("Windows 11 Pro (build 22631)"));
  assert_eq!(found.version.as_deref(), Some("10.0.22631"));
  assert_eq!(
    (cpu.model.as_str(), cpu.cores, cpu.logical),
    ("AMD Ryzen 7 5800X 8-Core Processor", Some(8.0), Some(16.0))
  );
  assert_eq!(found.swap, Some((4096.0 * 1024.0, 3584.0 * 1024.0)));
}

/// The script a command carries, unpacked.
fn carried(command: &str) -> String {
  let encoded = command.strip_prefix("powershell -NoProfile -NonInteractive -EncodedCommand ").unwrap();
  let units: Vec<u16> = STANDARD
    .decode(encoded)
    .unwrap()
    .chunks(2)
    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
    .collect();
  let unpack = String::from_utf16(&units).unwrap();
  let packed = unpack.split('\'').nth(1).unwrap();
  let mut script = String::new();

  assert!(unpack.ends_with("ReadToEnd()))"));
  flate2::read::GzDecoder::new(STANDARD.decode(packed).unwrap().as_slice())
    .read_to_string(&mut script)
    .unwrap();

  script
}

#[test]
fn a_powershell_command_carries_the_script_compressed_inside_an_encoded_command() {
  assert_eq!(carried(&powershell_command("'@@end'")), "'@@end'");
}

#[test]
fn the_live_powershell_script_travels_whole_in_one_command_of_bounded_length() {
  let script = std::fs::read_to_string(
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("crates/hardware/probe-files/live-powershell.ps1"),
  )
  .unwrap();
  let command = powershell_command(&script);

  assert_eq!(carried(&command), script);
  assert!(command.len() < 16_000, "{} characters", command.len());
}
