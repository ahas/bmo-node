//! The configuration read from its file (docs/api.md, 「Configuration」): the callers it lets onto every
//! personal node as one joins the list.

use node_server::config::Config;

#[test]
fn the_callers_a_personal_node_joins_with_are_those_whose_table_says_personal_nodes() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("node.toml");

  std::fs::write(
    &path,
    "listen = \"127.0.0.1:0\"\ntoken = \"t\"\ndata_root = \"/data\"\n\n\
     [callers.desk-driver]\ntoken_file = \"desk\"\npersonal_nodes = true\n\n\
     [callers.mail-reader]\ntoken_file = \"mail\"\npersonal_nodes = true\n\n\
     [callers.other-driver]\ntoken_file = \"other\"\n",
  )
  .unwrap();

  assert_eq!(
    Config::read(&path).unwrap().personal_callers().unwrap().joined(),
    Some("desk-driver+mail-reader".to_owned())
  );
}
