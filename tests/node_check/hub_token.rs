//! A hub's control token, read out of its environment file for the one request that needs it.

use node_check::Machine;
use node_check::hub::hub_token;

fn token_from(text: Option<&str>) -> Result<String, String> {
  let home = tempfile::tempdir().unwrap();

  if let Some(text) = text {
    std::fs::write(home.path().join("hub.env"), text).unwrap();
  }

  hub_token(&Machine::new("ssh", home.path()), "~/hub.env")
}

#[test]
fn reads_one_key_out_of_an_environment_file_quotes_taken_off() {
  assert_eq!(
    token_from(Some("JAKE_SECRET=other\nJAKE_CONTROL_TOKEN=\"secret-token-value\"\n")),
    Ok("secret-token-value".to_owned())
  );
  assert_eq!(
    token_from(Some("# JAKE_CONTROL_TOKEN=no\n")),
    Err("the token file ~/hub.env holds no JAKE_CONTROL_TOKEN".to_owned())
  );
  assert_eq!(token_from(None), Err("the token file ~/hub.env cannot be read".to_owned()));
}
