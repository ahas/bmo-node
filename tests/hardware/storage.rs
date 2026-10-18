//! Every storage device as its own piece of hardware: a Linux machine's mounts hung on the disks they
//! stand on, WSL's virtual disk alone, and a Windows machine's disks with their lettered volumes.

use hardware::probe::Sensor;
use hardware::storage::{StorageDevice, StorageVolume, read_linux_storage, windows_storage};
use hardware::windows::{WindowsDisk, WindowsVolume};

fn lines(records: &[&str]) -> Vec<String> {
  records.iter().map(|one| (*one).to_owned()).collect()
}

const BLK: &[&str] = &[
  "NAME=\"nvme0n1\" PKNAME=\"\" TYPE=\"disk\" SIZE=\"1000204886016\" MODEL=\"Samsung SSD 9100 PRO 1TB\" ROTA=\"0\" TRAN=\"nvme\"",
  "NAME=\"nvme0n1p1\" PKNAME=\"nvme0n1\" TYPE=\"part\" SIZE=\"1073741824\" MODEL=\"\" ROTA=\"0\" TRAN=\"nvme\"",
  "NAME=\"nvme0n1p3\" PKNAME=\"nvme0n1\" TYPE=\"part\" SIZE=\"816043786240\" MODEL=\"\" ROTA=\"0\" TRAN=\"nvme\"",
  "NAME=\"vgssd-root\" PKNAME=\"nvme0n1p3\" TYPE=\"lvm\" SIZE=\"996973477888\" MODEL=\"\" ROTA=\"0\" TRAN=\"\"",
  "NAME=\"sda\" PKNAME=\"\" TYPE=\"disk\" SIZE=\"3000592982016\" MODEL=\"ST3000DM007-1WY1\" ROTA=\"1\" TRAN=\"sata\"",
  "NAME=\"sdb\" PKNAME=\"\" TYPE=\"disk\" SIZE=\"8001563221504\" MODEL=\"Expansion Desk\" ROTA=\"1\" TRAN=\"usb\"",
  "NAME=\"loop0\" PKNAME=\"\" TYPE=\"loop\" SIZE=\"4096\" MODEL=\"\" ROTA=\"0\" TRAN=\"\"",
];

const DF: &[&str] = &[
  "/dev/mapper/vgssd-root ext4 957199192 462756020 447513720 51% /",
  "/dev/nvme0n1p1 vfat 1046508 8572 1037936 1% /boot/efi",
  "/dev/sda xfs 2928835792 1825852096 1102983696 63% /mnt/data 1",
  "/dev/sda xfs 2928835792 1825852096 1102983696 63% /srv/bind",
  "tmpfs tmpfs 8168544 4 8168540 1% /run",
  "/dev/loop0 squashfs 128 128 0 100% /snap/core/1",
  "share:/srv/repos nfs4 960301056 780949504 130497536 86% /home/user/repos",
  "cloud: fuse.rclone 1078984704 38063312 1040921392 4% /home/user/Cloud",
];

/// One device as the test reads it: its key, its kind, its temperature and the mounts on it.
type Read = (String, Option<&'static str>, Option<f64>, Vec<String>);

fn sensor(chip: &str, device: &str, label: &str, celsius: f64) -> Sensor {
  Sensor {
    chip: chip.into(),
    device: Some(device.into()),
    label: label.into(),
    celsius,
    limit_celsius: None,
  }
}

#[test]
fn hangs_each_mount_on_the_disk_it_stands_on_with_the_disk_s_own_temperature() {
  let found = read_linux_storage(
    &lines(BLK),
    &lines(&["nvme0n1 nvme0", "sda 0:0:0:0", "sdb 1:0:0:0"]),
    &lines(DF),
    &[
      sensor("nvme", "nvme0", "Sensor 1", 50.0),
      sensor("nvme", "nvme0", "Composite", 46.9),
      sensor("drivetemp", "0:0:0:0", "temp1", 34.0),
    ],
    false,
  );
  let read: Vec<Read> = found
    .iter()
    .map(|one| {
      (
        one.key.clone(),
        one.kind,
        one.celsius,
        one.volumes.iter().map(|volume| volume.mount.clone()).collect(),
      )
    })
    .collect();
  let owned = |mounts: &[&str]| mounts.iter().map(|one| (*one).to_owned()).collect::<Vec<_>>();

  assert_eq!(
    read,
    [
      ("nvme0n1".to_owned(), Some("nvme"), Some(46.9), owned(&["/", "/boot/efi"])),
      ("sda".to_owned(), Some("hdd"), Some(34.0), owned(&["/mnt/data 1"])),
      // Connected with nothing mounted is still connected.
      ("sdb".to_owned(), Some("usb"), None, owned(&[])),
      ("net:share:/srv/repos".to_owned(), Some("network"), None, owned(&["/home/user/repos"])),
      ("net:cloud:".to_owned(), Some("network"), None, owned(&["/home/user/Cloud"])),
    ]
  );
}

#[test]
fn on_wsl_only_the_virtual_disk_it_runs_from_counts_and_windows_own_drives_are_left_to_windows() {
  let found = read_linux_storage(
    &lines(&[
      "NAME=\"sdd\" PKNAME=\"\" TYPE=\"disk\" SIZE=\"1099511627776\" MODEL=\"Virtual Disk\" ROTA=\"1\" TRAN=\"\"",
      "NAME=\"sdc\" PKNAME=\"\" TYPE=\"disk\" SIZE=\"4294967296\" MODEL=\"Virtual Disk\" ROTA=\"1\" TRAN=\"\"",
    ]),
    &[],
    &lines(&[
      "/dev/sdd ext4 1055762868 25878468 976180928 3% /",
      "C:\\ 9p 242591740 190634848 51956892 79% /mnt/c",
      "none tmpfs 1 1 1 1% /mnt/wsl",
    ]),
    &[],
    true,
  );

  assert_eq!(
    found,
    [StorageDevice {
      key: "sdd".into(),
      name: "Virtual Disk".into(),
      kind: Some("virtual"),
      size_bytes: Some(1_099_511_627_776.0),
      celsius: None,
      limit_celsius: None,
      volumes: vec![StorageVolume {
        mount: "/".into(),
        fs_type: Some("ext4".into()),
        total_kb: 1_055_762_868.0,
        used_kb: 25_878_468.0,
        available_kb: 976_180_928.0,
      }],
    }]
  );
}

#[test]
fn names_each_windows_disk_by_its_bus_and_medium_and_keeps_its_lettered_volumes() {
  let found = windows_storage(&[
    WindowsDisk {
      number: 3.0,
      model: "SAMSUNG MZVLW256".into(),
      media: Some("SSD".into()),
      bus: Some("NVMe".into()),
      size_bytes: Some(256e9),
      celsius: Some(32.0),
      volumes: vec![WindowsVolume {
        mount: "D:".into(),
        fs_type: Some("NTFS".into()),
        total_kb: 1000.0,
        available_kb: 400.0,
      }],
    },
    WindowsDisk {
      number: 0.0,
      model: "ST2000DM006".into(),
      media: Some("HDD".into()),
      bus: Some("SATA".into()),
      size_bytes: Some(2e12),
      celsius: None,
      volumes: vec![],
    },
  ]);

  assert_eq!(
    found.iter().map(|one| (one.key.as_str(), one.kind, one.celsius)).collect::<Vec<_>>(),
    [("win3", Some("nvme"), Some(32.0)), ("win0", Some("hdd"), None)]
  );
  assert_eq!(
    found[0].volumes,
    [StorageVolume {
      mount: "D:".into(),
      fs_type: Some("NTFS".into()),
      total_kb: 1000.0,
      used_kb: 600.0,
      available_kb: 400.0,
    }]
  );
}
