//! One machine's reading as a screen draws it (docs/hardware.md): one group per piece
//! of hardware, each carrying its own use, capacity, temperature and power; and the figures of a reading
//! under the names the record keeps them by (「History」). Every figure is what the machine answered or
//! none, which a screen words as unreadable. Each temperature carries the limit its part names for it; the
//! screen works out every colour.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use super::chips::chip_view;
use super::js::{round, round_to};
use super::probe::{Gpu, Memory, Sensor, ServiceCount, Snapshot, Watts};
use super::storage::{StorageDevice, is_drive_sensor};
pub use bmo_node_client::hardware::{
  BoardView, CpuView, GpuView, HardwareView, MemoryView, ServicesView, StorageView, VolumeView,
};

fn share_of(part: Option<f64>, whole: Option<f64>) -> Option<f64> {
  let (part, whole) = (part?, whole?);

  (whole > 0.0).then(|| (part / whole).clamp(0.0, 1.0))
}

// The CPU's own sensors, by the drivers that read the processor itself.
const CPU_CHIPS: [&str; 3] = ["k10temp", "coretemp", "zenpower"];

static PACKAGE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^Package id \d+$").expect("the package pattern"));

fn package_sensor(sensors: &[Sensor]) -> Option<&Sensor> {
  if let Some(named) = sensors
    .iter()
    .find(|one| one.label == "Tctl" || one.label == "Tdie" || PACKAGE.is_match(&one.label))
  {
    return Some(named);
  }

  sensors.iter().fold(None, |top, one| match top {
    Some(top) if one.celsius <= top.celsius => Some(top),
    _ => Some(one),
  })
}

fn cpu_view(snapshot: &Snapshot) -> CpuView {
  let sensors: Vec<Sensor> = snapshot
    .sensors
    .iter()
    .filter(|one| CPU_CHIPS.contains(&one.chip.as_str()))
    .cloned()
    .collect();
  let clocks: Vec<f64> = snapshot.threads.iter().filter_map(|one| one.mhz).collect();
  let named = package_sensor(&sensors);
  let (celsius, limit_celsius) = (named.map(|one| one.celsius), named.and_then(|one| one.limit_celsius));

  CpuView {
    model: snapshot.cpu_model.clone(),
    thread_count: snapshot.threads.len(),
    usage: snapshot.cpu_usage,
    celsius,
    limit_celsius,
    mhz: (!clocks.is_empty()).then(|| round(clocks.iter().sum::<f64>() / clocks.len() as f64)),
    peak_mhz: clocks.iter().copied().reduce(f64::max),
    watts: snapshot.cpu_watts,
    chip: chip_view(snapshot.cpu_model.as_deref(), &snapshot.threads, &snapshot.sensors),
    sensors,
  }
}

fn memory_view(memory: Option<&Memory>) -> MemoryView {
  let used_kb = memory.map(|memory| memory.total_kb - memory.available_kb);
  let share = share_of(used_kb, memory.map(|memory| memory.total_kb));
  let swap_total = memory.map(|memory| memory.swap_total_kb).filter(|total| *total != 0.0);
  let swap_used = swap_total.zip(memory).map(|(total, memory)| total - memory.swap_free_kb);

  MemoryView {
    used_kb,
    total_kb: memory.map(|memory| memory.total_kb),
    share,
    swap_used_kb: swap_used,
    swap_total_kb: swap_total,
    swap_share: share_of(swap_used, swap_total),
  }
}

fn gpu_view(gpu: &Gpu) -> GpuView {
  let watt_share = share_of(gpu.watts, gpu.watt_limit);
  let memory_share = share_of(gpu.memory_used_mb, gpu.memory_total_mb);

  GpuView {
    index: gpu.index,
    name: gpu.name.clone(),
    usage: gpu.usage,
    celsius: gpu.celsius,
    limit_celsius: gpu.limit_celsius,
    throttling: gpu.throttling,
    mhz: gpu.mhz,
    max_mhz: gpu.max_mhz,
    memory_mhz: gpu.memory_mhz,
    watts: gpu.watts,
    watt_limit: gpu.watt_limit,
    watt_share,
    memory_used_mb: gpu.memory_used_mb,
    memory_total_mb: gpu.memory_total_mb,
    memory_share,
    fan_percent: gpu.fan_percent,
  }
}

fn storage_view(device: &StorageDevice) -> StorageView {
  let volumes: Vec<VolumeView> = device
    .volumes
    .iter()
    .map(|one| {
      let share = share_of(Some(one.used_kb), Some(one.used_kb + one.available_kb)).unwrap_or(0.0);

      VolumeView {
        volume: one.clone(),
        share,
      }
    })
    .collect();
  let used_kb = (!volumes.is_empty()).then(|| volumes.iter().map(|one| one.volume.used_kb).sum());
  let total_kb = (!volumes.is_empty())
    .then(|| volumes.iter().map(|one| one.volume.used_kb + one.volume.available_kb).sum());
  let share = share_of(used_kb, total_kb);

  StorageView {
    key: device.key.clone(),
    name: device.name.clone(),
    kind: device.kind.map(str::to_owned),
    size_bytes: device.size_bytes,
    celsius: device.celsius,
    limit_celsius: device.limit_celsius,
    volumes,
    used_kb,
    total_kb,
    share,
  }
}

fn services_view(services: &(Option<ServiceCount>, Option<ServiceCount>)) -> ServicesView {
  let (system, user) = services;

  if system.is_none() && user.is_none() {
    return ServicesView {
      running: None,
      failed: None,
    };
  }

  let running =
    system.as_ref().map_or(0.0, |one| one.running) + user.as_ref().map_or(0.0, |one| one.running);
  let mut failed: Vec<String> = system.as_ref().map(|one| one.failed.clone()).unwrap_or_default();

  failed.extend(user.iter().flat_map(|one| one.failed.iter().map(|failed| format!("{failed} (user)"))));

  ServicesView {
    running: Some(running),
    failed: Some(failed),
  }
}

pub fn hardware_view(snapshot: &Snapshot) -> HardwareView {
  HardwareView {
    host: snapshot.host.clone(),
    kernel: snapshot.kernel.clone(),
    os: snapshot.os.clone(),
    host_os: snapshot.host_os.clone(),
    wsl: snapshot.wsl,
    sensor_library: snapshot.sensor_library.map(str::to_owned),
    uptime_seconds: snapshot.uptime_seconds,
    load: snapshot.load,
    cpu: cpu_view(snapshot),
    memory: memory_view(snapshot.memory.as_ref()),
    gpus: snapshot.gpus.iter().map(gpu_view).collect(),
    storage: snapshot.storage.iter().map(storage_view).collect(),
    board: BoardView {
      sensors: snapshot
        .sensors
        .iter()
        .filter(|one| !CPU_CHIPS.contains(&one.chip.as_str()) && !is_drive_sensor(one))
        .cloned()
        .collect(),
      fans: snapshot.fans.clone(),
    },
    services: services_view(&snapshot.services),
  }
}

/// A figure as JSON, without a point where it is whole, as the console writes one.
fn figure(value: f64) -> Value {
  if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
    json!(value as i64)
  } else {
    json!(value)
  }
}

/// One reading's figures under their series names — the piece of hardware and what it measures, `|`
/// between them — to a tenth, a store to a hundredth of a gigabyte. What was not read is left out
/// rather than kept as nought.
pub fn history_sample(view: &HardwareView) -> Map<String, Value> {
  let mut sample = Map::new();
  let mut put = |device: &str, metric: &str, value: Option<f64>| {
    let Some(value) = value.filter(|value| value.is_finite()) else {
      return;
    };
    let places = if metric == "gb" { 100.0 } else { 10.0 };

    sample.insert(format!("{device}|{metric}"), figure(round_to(value, places)));
  };
  let percent = |share: Option<f64>| share.map(|share| share * 100.0);
  let gb_of_kb = |kb: Option<f64>| kb.map(|kb| kb / 1024.0 / 1024.0);

  put("cpu", "use", percent(view.cpu.usage));
  put("cpu", "temp", view.cpu.celsius);
  put(
    "cpu",
    "watts",
    match view.cpu.watts {
      Some(Watts::Read(watts)) => Some(watts),
      _ => None,
    },
  );
  put("cpu", "mhz", view.cpu.mhz);
  put("memory", "gb", gb_of_kb(view.memory.used_kb));

  for gpu in &view.gpus {
    let device = format!("gpu{}", super::js::shown(gpu.index));

    put(&device, "use", percent(gpu.usage));
    put(&device, "temp", gpu.celsius);
    put(&device, "watts", gpu.watts);
    put(&device, "mhz", gpu.mhz);
    put(&format!("{device}:vram"), "gb", gpu.memory_used_mb.map(|mb| mb / 1024.0));
  }

  for device in &view.storage {
    put(&format!("storage:{}", device.key), "gb", gb_of_kb(device.used_kb));
    put(&format!("storage:{}", device.key), "temp", device.celsius);
  }

  for fan in &view.board.fans {
    put(&format!("fan:{}:{}", fan.chip, fan.label), "rpm", Some(fan.rpm));
  }

  sample
}
