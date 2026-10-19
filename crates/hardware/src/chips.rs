//! The chip as it physically stands (docs/hardware.md, 「The chip」). The CPU model names the layout —
//! an external identifier, which is why the families are a table of patterns — and the machine's own
//! topology says how many of each part it holds. Every part carries the figures a screen colours, and
//! the screen works out the colours.

use std::sync::LazyLock;

use regex::Regex;

use super::js::{round, shown};
use super::probe::{CpuThread, Sensor};
pub use bmo_node_client::hardware::{ChipView, CoreView, DieView, Item, ThreadUse};

#[derive(Clone)]
struct Core {
  core: Option<f64>,
  threads: Vec<CpuThread>,
}

/// The machine's cores in the order of their numbers, each with its threads.
fn cores_of(threads: &[CpuThread]) -> Vec<Core> {
  let mut found: Vec<(String, Core)> = Vec::new();

  for thread in threads {
    let key = match thread.core {
      None => format!("cpu{}", shown(thread.cpu)),
      Some(core) => format!("{}:{}", shown(thread.package.unwrap_or(0.0)), shown(core)),
    };

    match found.iter_mut().find(|(one, _)| *one == key) {
      Some((_, core)) => core.threads.push(thread.clone()),
      None => found.push((
        key,
        Core {
          core: thread.core,
          threads: vec![thread.clone()],
        },
      )),
    }
  }

  let mut cores: Vec<Core> = found.into_iter().map(|(_, core)| core).collect();
  let package = |core: &Core| core.threads[0].package.unwrap_or(0.0);
  let place = |core: &Core| core.core.unwrap_or(core.threads[0].cpu);

  cores.sort_by(|a, b| package(a).total_cmp(&package(b)).then_with(|| place(a).total_cmp(&place(b))));
  cores
}

fn mean(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
  let known: Vec<f64> = values.flatten().collect();

  (!known.is_empty()).then(|| known.iter().sum::<f64>() / known.len() as f64)
}

fn core_view(core: &Core, index: usize, celsius: Option<f64>) -> CoreView {
  let usage = mean(core.threads.iter().map(|one| one.usage));

  CoreView {
    label: format!("C{}", core.core.map_or_else(|| index.to_string(), shown)),
    usage,
    mhz: core.threads.iter().filter_map(|one| one.mhz).reduce(f64::max),
    celsius,
    threads: core
      .threads
      .iter()
      .map(|one| ThreadUse {
        cpu: one.cpu,
        usage: one.usage,
      })
      .collect(),
  }
}

static CACHE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)^(\d+)\s*([KM])?").expect("the cache pattern"));

/// `16384K` as a person reads a cache.
pub fn cache_size(text: Option<&str>) -> Option<String> {
  let found = CACHE.captures(text?.trim())?;
  let kb = found[1].parse::<f64>().ok()?
    * if found.get(2).is_some_and(|unit| unit.as_str().eq_ignore_ascii_case("m")) {
      1024.0
    } else {
      1.0
    };

  Some(if kb >= 1024.0 {
    format!("{} MB", shown(round(kb / 1024.0)))
  } else {
    format!("{} KB", shown(kb))
  })
}

fn split(cores: Vec<CoreView>) -> (Vec<CoreView>, Vec<CoreView>) {
  let half = cores.len().div_ceil(2);
  let mut left = cores;
  let right = left.split_off(half.min(left.len()));

  (left, right)
}

fn chunk<T: Clone>(items: &[T], count: usize) -> Vec<Vec<T>> {
  let size = items.len().div_ceil(count);

  (0..count)
    .map(|index| items.iter().skip(index * size).take(size).cloned().collect::<Vec<_>>())
    .filter(|one| !one.is_empty())
    .collect()
}

fn cache_label(name: &str, cores: &[Core]) -> String {
  match cache_size(cores.first().and_then(|core| core.threads[0].l3_size.as_deref())) {
    Some(size) => format!("{name} {size}"),
    None => name.to_owned(),
  }
}

/// What the sensors say about the package, each die and each core.
#[derive(Default)]
struct Temperatures {
  package: Option<f64>,
  /// By die index, from AMD's Tccd<n>.
  dies: Vec<(f64, f64)>,
  /// By core number, from Intel's Core <n>.
  cores: Vec<(f64, f64)>,
}

fn set(table: &mut Vec<(f64, f64)>, key: f64, value: f64) {
  match table.iter_mut().find(|(one, _)| *one == key) {
    Some(known) => known.1 = value,
    None => table.push((key, value)),
  }
}

fn get(table: &[(f64, f64)], key: f64) -> Option<f64> {
  table.iter().find(|(one, _)| *one == key).map(|(_, value)| *value)
}

static TCCD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Tccd(\d+)$").expect("the Tccd pattern"));
static INTEL_CORE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^Core (\d+)$").expect("the core pattern"));
static PACKAGE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^Package id \d+$").expect("the package pattern"));

fn temperatures(sensors: &[Sensor]) -> Temperatures {
  let mut found = Temperatures::default();
  let package = |celsius: f64, found: &mut Temperatures| {
    found.package = Some(found.package.unwrap_or(celsius).max(celsius));
  };

  for sensor in sensors {
    if sensor.chip == "k10temp" || sensor.chip == "zenpower" {
      if let Some(ccd) = TCCD.captures(&sensor.label) {
        set(&mut found.dies, ccd[1].parse::<f64>().unwrap_or(0.0) - 1.0, sensor.celsius);
      } else if sensor.label == "Tctl" || sensor.label == "Tdie" {
        package(sensor.celsius, &mut found);
      }
    } else if sensor.chip == "coretemp" {
      if let Some(core) = INTEL_CORE.captures(&sensor.label) {
        set(&mut found.cores, core[1].parse().unwrap_or(0.0), sensor.celsius);
      } else if PACKAGE.is_match(&sensor.label) {
        package(sensor.celsius, &mut found);
      }
    }
  }

  found
}

struct Layout {
  model: String,
  cores: Vec<Core>,
  temps: Temperatures,
}

fn views(cores: &[Core], temps: &Temperatures) -> Vec<CoreView> {
  cores
    .iter()
    .enumerate()
    .map(|(index, core)| core_view(core, index, core.core.and_then(|number| get(&temps.cores, number))))
    .collect()
}

fn compute_die(label: String, celsius: Option<f64>, direction: &'static str, items: Vec<Item>) -> DieView {
  DieView {
    label,
    role: "compute".into(),
    celsius,
    direction: direction.into(),
    items,
  }
}

fn cluster(label: String, cache: String, cores: Vec<CoreView>) -> Item {
  let (left, right) = split(cores);

  Item::Cluster {
    label,
    cache,
    left,
    right,
  }
}

fn part(label: &str, off: bool) -> Item {
  Item::Part {
    label: label.to_owned(),
    off,
  }
}

/// AM4's chiplet package: up to two CCD places side by side above the I/O die.
fn am4(
  layout: &Layout,
  per_die: impl Fn(&[Core], usize) -> Vec<Item>,
  direction: &'static str,
) -> ChipView {
  let mut dies = chunk(&layout.cores, if layout.cores.len() > 8 { 2 } else { 1 })
    .iter()
    .enumerate()
    .map(|(die, cores)| {
      compute_die(format!("CCD{die}"), get(&layout.temps.dies, die as f64), direction, per_die(cores, die))
    })
    .collect::<Vec<_>>()
    .into_iter();
  let io = DieView {
    label: "IOD".into(),
    role: "io".into(),
    celsius: None,
    direction: "column".into(),
    items: vec![part("Memory · PCIe", false)],
  };

  ChipView {
    label: layout.model.clone(),
    celsius: layout.temps.package,
    rows: vec![vec![dies.next(), dies.next()], vec![Some(io)]],
  }
}

/// A Zen 2 CCD: two CCXs one above the other with the fabric between them, each CCX's cores half on
/// either side of its L3.
fn zen2(layout: &Layout) -> ChipView {
  am4(
    layout,
    |cores, die| {
      let mut halves = chunk(cores, 2).into_iter();
      let top = halves.next().unwrap_or_default();
      let bottom = halves.next().unwrap_or_default();
      let ccx = |group: &[Core], index: usize| {
        cluster(format!("CCX{}", die * 2 + index), cache_label("L3", group), views(group, &layout.temps))
      };

      vec![ccx(&top, 0), part("Infinity Fabric", false), ccx(&bottom, 1)]
    },
    "column",
  )
}

/// A Zen 3 CCD: one CCX, a column of cores either side of the L3.
fn zen3(layout: &Layout) -> ChipView {
  am4(
    layout,
    |cores, die| vec![cluster(format!("CCX{die}"), cache_label("L3", cores), views(cores, &layout.temps))],
    "column",
  )
}

static FUSED: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\d{4,5}[A-Z]*F\b").expect("the fused pattern"));

/// An Intel ring die: the GPU on the left, every core beside its LLC slice, the system agent on the right.
fn intel_ring(layout: &Layout) -> ChipView {
  let die = compute_die(
    "Die".into(),
    layout.temps.package,
    "row",
    vec![
      part("GPU", FUSED.is_match(&layout.model)),
      Item::Cluster {
        label: "Ring".into(),
        cache: cache_label("LLC", &layout.cores),
        left: views(&layout.cores, &layout.temps),
        right: vec![],
      },
      part("System Agent", false),
    ],
  );

  ChipView {
    label: layout.model.clone(),
    celsius: layout.temps.package,
    rows: vec![vec![Some(die)]],
  }
}

/// One die, the cores grouped by the L3 they share, each group around its L3.
fn generic(layout: &Layout) -> ChipView {
  let mut groups: Vec<(String, Vec<Core>)> = Vec::new();

  for core in &layout.cores {
    let key = core.threads[0].l3.clone().unwrap_or_default();

    match groups.iter_mut().find(|(one, _)| *one == key) {
      Some((_, cores)) => cores.push(core.clone()),
      None => groups.push((key, vec![core.clone()])),
    }
  }

  let several = groups.len() > 1;
  let items = groups
    .iter()
    .enumerate()
    .map(|(index, (_, cores))| {
      cluster(
        if several { format!("L3 {index}") } else { "Cores".into() },
        cache_label("L3", cores),
        views(cores, &layout.temps),
      )
    })
    .collect();

  ChipView {
    label: layout.model.clone(),
    celsius: layout.temps.package,
    rows: vec![vec![Some(compute_die(
      "Die".into(),
      get(&layout.temps.dies, 0.0).or(layout.temps.package),
      "column",
      items,
    ))]],
  }
}

static ZEN2: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"AMD Ryzen \d+ (PRO )?3\d{3}(X|XT)?\b").expect("the Zen 2 pattern"));
static ZEN3: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"AMD Ryzen \d+ (PRO )?5\d{3}(X3D|XT|X)?\b").expect("the Zen 3 pattern"));
static INTEL_RING: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"Core\(TM\) i[3579]-(\d{4,5})").expect("the Intel pattern"));

fn intel_ring_generation(model: &str) -> bool {
  let Some(number) = INTEL_RING.captures(model).map(|found| found[1].to_owned()) else {
    return false;
  };
  let generation: u32 = if number.len() == 5 {
    number[..2].parse().unwrap_or(0)
  } else {
    number[..1].parse().unwrap_or(0)
  };

  (2..=10).contains(&generation)
}

/// The chip of the threads and sensors read, in the first family whose pattern its model matches.
pub fn chip_view(model: Option<&str>, threads: &[CpuThread], sensors: &[Sensor]) -> Option<ChipView> {
  if threads.is_empty() {
    return None;
  }

  let layout = Layout {
    model: model.unwrap_or("CPU").to_owned(),
    cores: cores_of(threads),
    temps: temperatures(sensors),
  };

  Some(match model {
    Some(model) if ZEN2.is_match(model) => zen2(&layout),
    Some(model) if ZEN3.is_match(model) => zen3(&layout),
    Some(model) if intel_ring_generation(model) => intel_ring(&layout),
    _ => generic(&layout),
  })
}
