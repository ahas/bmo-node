//! Every storage device a machine has, each with the volumes that stand on it (docs/hardware.md,
//! 「Storage」): a Linux machine's out of lsblk's pairs, df's columns and the drives' own hwmon sensors; a
//! WSL machine's the Windows disks with their lettered volumes, beside the virtual disk WSL runs from.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use super::js::number;
use super::probe::Sensor;
use super::windows::WindowsDisk;
pub use bmo_node_client::hardware::StorageVolume;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageDevice {
  /// Unique within the machine: the kernel's name, `win<n>` for a Windows disk, `net:<source>` for a
  /// network share.
  pub key: String,
  /// The drive's model, or the share's source.
  pub name: String,
  pub kind: Option<&'static str>,
  pub size_bytes: Option<f64>,
  pub celsius: Option<f64>,
  /// The limit the drive's sensor names for its temperature (docs/hardware.md, 「Temperature limits」).
  pub limit_celsius: Option<f64>,
  pub volumes: Vec<StorageVolume>,
}

struct Block {
  name: String,
  parent: Option<String>,
  kind: String,
  size_bytes: Option<f64>,
  model: Option<String>,
  rotational: bool,
  transport: Option<String>,
}

// File systems that are the kernel's own or a view of another one, never a device's.
const NOT_STORAGE: [&str; 20] = [
  "tmpfs",
  "devtmpfs",
  "overlay",
  "squashfs",
  "efivarfs",
  "rootfs",
  "proc",
  "sysfs",
  "ramfs",
  "9p",
  "drvfs",
  "nsfs",
  "autofs",
  "fuse.snapfuse",
  "fuse.portal",
  "fuse.gvfsd-fuse",
  "cgroup2",
  "devpts",
  "tracefs",
  "debugfs",
];

/// The file systems that are a network share; the probe asks df for these beside the ones on a block
/// device.
static NETWORK_FILE_SYSTEM: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(nfs4?|cifs|smb3|fuse\.(rclone|sshfs))$").expect("the network pattern"));
static PAIR: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r#"(\w+)="([^"]*)""#).expect("the pair pattern"));
static VIRTUAL_DISK: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)virtual disk").expect("the virtual pattern"));
static NOT_A_DISK: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(loop|zram|ram)").expect("the not-a-disk pattern"));
static WSL_OWN: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^/(mnt/wsl|usr/lib/wsl|init$)").expect("the WSL mount pattern"));

fn blocks(lines: &[String]) -> Vec<Block> {
  lines
    .iter()
    .filter_map(|line| {
      let mut cells: Vec<(String, String)> = Vec::new();

      for found in PAIR.captures_iter(line) {
        let pair = (found[1].to_owned(), found[2].trim().to_owned());

        match cells.iter().position(|(key, _)| *key == pair.0) {
          Some(at) => cells[at] = pair,
          None => cells.push(pair),
        }
      }

      let cell = |key: &str| cells.iter().find(|(one, _)| one == key).map(|(_, value)| value.as_str());
      let filled = |key: &str| cell(key).filter(|value| !value.is_empty()).map(str::to_owned);
      let name = filled("NAME")?;
      let size = cell("SIZE").and_then(number).filter(|size| *size > 0.0);

      Some(Block {
        name,
        parent: filled("PKNAME"),
        kind: cell("TYPE").unwrap_or_default().to_owned(),
        size_bytes: size,
        model: filled("MODEL"),
        rotational: cell("ROTA") == Some("1"),
        transport: filled("TRAN"),
      })
    })
    .collect()
}

fn linux_kind(disk: &Block, wsl: bool) -> &'static str {
  if wsl || disk.model.as_deref().is_some_and(|model| VIRTUAL_DISK.is_match(model)) {
    return "virtual";
  }

  if disk.transport.as_deref() == Some("nvme") || disk.name.starts_with("nvme") {
    return "nvme";
  }

  if disk.transport.as_deref() == Some("usb") {
    return "usb";
  }

  if disk.rotational { "hdd" } else { "ssd" }
}

/// The disk a block device stands on, following its parents; itself where it is a disk.
fn disk_of<'a>(name: &str, all: &'a [Block]) -> Option<&'a Block> {
  let by_name = |name: &str| all.iter().find(|one| one.name == name);
  let mut seen: Vec<&str> = Vec::new();
  let mut current = by_name(name);

  while let Some(one) = current {
    if seen.contains(&one.name.as_str()) {
      break;
    }

    if one.kind == "disk" {
      return Some(one);
    }

    seen.push(&one.name);
    current = one.parent.as_deref().and_then(by_name);
  }

  None
}

struct Mount {
  source: String,
  fs_type: String,
  total_kb: f64,
  used_kb: f64,
  available_kb: f64,
  mount: String,
}

fn mounts(lines: &[String]) -> Vec<Mount> {
  let mut seen: Vec<String> = Vec::new();

  lines
    .iter()
    .filter_map(|line| {
      let cells: Vec<&str> = line.split_whitespace().collect();

      if cells.len() < 7 {
        return None;
      }

      let values: Vec<f64> = cells[2..5].iter().map(|cell| number(cell)).collect::<Option<_>>()?;

      // A bind mount shows the same source again; the first place it stands is the one kept.
      if values[0] == 0.0 || seen.iter().any(|source| source == cells[0]) {
        return None;
      }

      seen.push(cells[0].to_owned());

      Some(Mount {
        source: cells[0].to_owned(),
        fs_type: cells[1].to_owned(),
        total_kb: values[0],
        used_kb: values[1],
        available_kb: values[2],
        mount: cells[6..].join(" "),
      })
    })
    .collect()
}

/// The chips whose readings belong to a drive rather than to the board.
pub fn is_drive_sensor(sensor: &Sensor) -> bool {
  sensor.chip == "nvme" || sensor.chip == "drivetemp"
}

/// A drive's own temperature sensor, out of those its hwmon chip reports: the composite where there are
/// several.
fn drive_sensor<'a>(device: Option<&str>, sensors: &'a [Sensor]) -> Option<&'a Sensor> {
  let device = device?;
  let own: Vec<&Sensor> = sensors
    .iter()
    .filter(|one| one.device.as_deref() == Some(device) && is_drive_sensor(one))
    .collect();

  own.iter().find(|one| one.label == "Composite").or(own.first()).copied()
}

fn put(devices: &mut Vec<StorageDevice>, device: StorageDevice) {
  match devices.iter().position(|one| one.key == device.key) {
    Some(at) => devices[at] = device,
    None => devices.push(device),
  }
}

pub fn read_linux_storage(
  blk: &[String],
  blkdev: &[String],
  df: &[String],
  sensors: &[Sensor],
  wsl: bool,
) -> Vec<StorageDevice> {
  let all = blocks(blk);
  // Each block device's sysfs device, which is what a drive's hwmon chip names as its own.
  let mut sys_device: Vec<(&str, &str)> = Vec::new();

  for line in blkdev {
    let cells: Vec<&str> = line.split_whitespace().collect();

    if let [name, device] = cells.as_slice() {
      match sys_device.iter().position(|(one, _)| one == name) {
        Some(at) => sys_device[at] = (name, device),
        None => sys_device.push((name, device)),
      }
    }
  }

  let mut devices: Vec<StorageDevice> = Vec::new();
  let device_of = |devices: &mut Vec<StorageDevice>, disk: &Block| -> usize {
    if let Some(at) = devices.iter().position(|one| one.key == disk.name) {
      return at;
    }

    let device = sys_device.iter().find(|(name, _)| *name == disk.name).map(|(_, device)| *device);

    let sensor = if wsl { None } else { drive_sensor(device, sensors) };

    devices.push(StorageDevice {
      key: disk.name.clone(),
      name: disk.model.clone().unwrap_or_else(|| disk.name.clone()),
      kind: Some(linux_kind(disk, wsl)),
      size_bytes: disk.size_bytes,
      celsius: sensor.map(|one| one.celsius),
      limit_celsius: sensor.and_then(|one| one.limit_celsius),
      volumes: vec![],
    });

    devices.len() - 1
  };

  for disk in &all {
    if disk.kind == "disk" && !wsl && !NOT_A_DISK.is_match(&disk.name) {
      device_of(&mut devices, disk);
    }
  }

  for mount in mounts(df) {
    if NOT_STORAGE.contains(&mount.fs_type.as_str()) || mount.source.starts_with("/dev/loop") {
      continue;
    }

    if wsl && WSL_OWN.is_match(&mount.mount) {
      continue;
    }

    let volume = StorageVolume {
      mount: mount.mount.clone(),
      fs_type: Some(mount.fs_type.clone()),
      total_kb: mount.total_kb,
      used_kb: mount.used_kb,
      available_kb: mount.available_kb,
    };

    if NETWORK_FILE_SYSTEM.is_match(&mount.fs_type) {
      put(
        &mut devices,
        StorageDevice {
          key: format!("net:{}", mount.source),
          name: mount.source.clone(),
          kind: Some("network"),
          size_bytes: Some(mount.total_kb * 1024.0),
          celsius: None,
          limit_celsius: None,
          volumes: vec![volume],
        },
      );
      continue;
    }

    if !mount.source.starts_with("/dev/") {
      continue;
    }

    match disk_of(mount.source.rsplit('/').next().unwrap_or_default(), &all) {
      Some(disk) => {
        let at = device_of(&mut devices, disk);

        devices[at].volumes.push(volume);
      }
      None => put(
        &mut devices,
        StorageDevice {
          key: mount.source.clone(),
          name: mount.source.clone(),
          kind: None,
          size_bytes: Some(mount.total_kb * 1024.0),
          celsius: None,
          limit_celsius: None,
          volumes: vec![volume],
        },
      ),
    }
  }

  devices
}

fn windows_kind(disk: &WindowsDisk) -> Option<&'static str> {
  match (disk.bus.as_deref(), disk.media.as_deref()) {
    (Some("NVMe"), _) => Some("nvme"),
    (Some("USB"), _) => Some("usb"),
    (_, Some("HDD")) => Some("hdd"),
    (_, Some("SSD")) => Some("ssd"),
    _ => None,
  }
}

pub fn windows_storage(disks: &[WindowsDisk]) -> Vec<StorageDevice> {
  disks
    .iter()
    .map(|disk| StorageDevice {
      key: format!("win{}", super::js::shown(disk.number)),
      name: disk.model.clone(),
      kind: windows_kind(disk),
      size_bytes: disk.size_bytes,
      celsius: disk.celsius,
      limit_celsius: None,
      volumes: disk
        .volumes
        .iter()
        .map(|one| StorageVolume {
          mount: one.mount.clone(),
          fs_type: one.fs_type.clone(),
          total_kb: one.total_kb,
          used_kb: one.total_kb - one.available_kb,
          available_kb: one.available_kb,
        })
        .collect(),
    })
    .collect()
}
