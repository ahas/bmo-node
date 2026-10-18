//! The node service's store: its tables made once with the seeded hub, the node list and the hubs read and
//! changed, a check written to its row, the settings, and the hardware history's rows.

mod history;
mod nodes;
mod settings;

use store::Store;

/// A store of its own in a scratch folder, held with the folder.
pub struct Scratch {
  pub dir: tempfile::TempDir,
  pub store: Store,
}

pub fn scratch() -> Scratch {
  let dir = tempfile::tempdir().unwrap();
  let store = Store::open_in(dir.path()).unwrap();

  Scratch { dir, store }
}

#[test]
fn the_store_and_the_live_record_stand_in_the_node_folder_of_the_data_root() {
  let root = std::path::Path::new("/data");

  assert_eq!(store::store_path(root), root.join("node/node.db"));
  assert_eq!(store::live_record_path(root), root.join("node/hardware-live.db"));

  let scratch = scratch();

  assert!(scratch.dir.path().join("node/node.db").exists());
}

#[test]
fn a_fresh_store_holds_the_seeded_hub_and_a_hub_removed_stays_removed() {
  let scratch = scratch();
  let hubs = scratch.store.hubs().unwrap();

  assert_eq!(hubs.len(), 1);
  assert_eq!((hubs[0].url.as_str(), hubs[0].token_file.as_str()), store::HUB_SEED);
  assert!(scratch.store.nodes().unwrap().is_empty());

  scratch.store.remove_hub(store::HUB_SEED.0).unwrap();

  let reopened = Store::open_in(scratch.dir.path()).unwrap();

  assert!(reopened.hubs().unwrap().is_empty());
}
