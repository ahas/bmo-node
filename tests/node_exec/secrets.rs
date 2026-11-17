//! Secrets handed by name through the tool (docs/tools.md, 「Secrets handed by name」): read from the space the
//! caller reads, laid into the hub's exec as variables or carried for a file, the command's text holding no
//! value, the folder removed again after the command's end, and every value masked in the answer. Each secret
//! here is a throwaway written into the stand-in's own data root.

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bmo_node_client::nodes::LoginShell;
use node_exec_tool::answer;
use serde_json::{Value, json};

use super::{Stood, frame, handed, hub_target, out, owned};

const USER: &str = "tester";
const PASSWORD: &str = "throwaway-pass-7731";
const TOKEN: &str = "throwaway-token-0042";

fn write(folder: &Path, name: &str, text: &str) {
  std::fs::create_dir_all(folder).unwrap();
  std::fs::write(folder.join(name), text).unwrap();
}

/// The stand-in with account 4's node, a login of fields and a token of one value in the platform's space, and
/// a token of account 4's own; the hub answering every exec with `said` on its output and `code` as its end.
async fn stocked(said: &str, code: i64) -> Stood {
  let stood = owned().await;

  write(&stood.env, "site-login", &format!("USERNAME={USER}\nPASSWORD={PASSWORD}\n"));
  write(&stood.env, "site-token", TOKEN);
  write(&stood.env.join("users/4"), "own-token", "throwaway-own-5150");
  *stood.world.frames.lock().unwrap() = vec![
    frame("started", json!({ "id": "c1" })),
    out("stdout", said.as_bytes(), 0),
    frame("exit", json!({ "code": code, "signal": null, "canceled": false })),
  ];
  stood
}

fn bodies(stood: &Stood) -> Vec<Value> {
  stood.world.execs.lock().unwrap().iter().map(|(_, _, body)| body.clone()).collect()
}

#[tokio::test]
async fn a_login_is_handed_as_variables_and_its_values_are_masked_in_the_answer() {
  let stood = stocked(&format!("signed in as {USER} with {PASSWORD}\n"), 0).await;
  let arguments = json!({
    "node": "pc-four",
    "command": "./sign-in.ps1",
    "env": { "MODE": "quiet" },
    "secrets": [{ "name": "site-login", "prefix": "SITE_" }],
  });
  let answered = answer(&handed("house", &["node.admin"], arguments), &stood.ground).await;

  assert!(!answered.is_error, "{}", answered.text());
  assert_eq!(
    bodies(&stood),
    [json!({
      "shell": "powershell",
      "command": "./sign-in.ps1",
      "env": { "MODE": "quiet", "SITE_USERNAME": USER, "SITE_PASSWORD": PASSWORD },
    })]
  );

  let rendered = serde_json::to_string(&answered).unwrap();

  assert!(!rendered.contains(PASSWORD) && !rendered.contains(USER), "{rendered}");
  assert_eq!(
    answered.structured.unwrap()["stdout"],
    json!("signed in as [secret site-login] with [secret site-login]\n")
  );
}

#[tokio::test]
async fn a_token_is_carried_for_a_file_its_folder_removed_again_after_the_command_s_end() {
  let stood = stocked(&format!("{TOKEN}\n"), 0).await;
  let arguments = json!({ "node": "pc-four", "command": "Get-Content $env:BMO_SECRET_SITE_TOKEN_FILE", "secrets": ["site-token"] });
  let answered = answer(&handed("house", &["node.admin"], arguments), &stood.ground).await;

  assert!(!answered.is_error, "{}", answered.text());
  assert_eq!(answered.text(), "pc-four (powershell): exit code 0\n--- stdout\n[secret site-token]\n");

  let [ran, removal] = bodies(&stood).try_into().unwrap();
  let command = ran["command"].as_str().unwrap();
  let encoded = STANDARD.encode(TOKEN);
  let removing = removal["command"].as_str().unwrap();
  let folder = removing.split("'bmo-secret-").nth(1).unwrap().split('\'').next().unwrap();

  assert_eq!(ran["env"], json!({ "BMO_SECRET_BYTES_0": encoded }));
  assert!(command.contains("$env:BMO_SECRET_SITE_TOKEN_FILE = "), "{command}");
  assert!(command.contains("Get-Content $env:BMO_SECRET_SITE_TOKEN_FILE"), "{command}");
  assert!(!command.contains(TOKEN) && !command.contains(&encoded), "{command}");
  assert_eq!(folder.len(), 32, "{removing}");
  assert!(command.contains(&format!("'bmo-secret-{folder}'")), "{command}");
  assert_eq!(removal["shell"], json!("powershell"));
  assert_eq!(removal["env"], json!({}));
}

#[tokio::test]
async fn a_removal_that_does_not_end_with_0_is_said_with_the_answer() {
  let stood = stocked("done\n", 3).await;
  let target = hub_target("pc-four", &stood.url, "agent_w", LoginShell::Posix);

  stood.world.targets.lock().unwrap().insert("pc-four".into(), target);

  let arguments =
    json!({ "node": "pc-four", "command": "true", "secrets": [{ "name": "site-login", "as": "file" }] });
  let answered = answer(&handed("house", &["node.admin"], arguments), &stood.ground).await;
  let [ran, removal] = bodies(&stood).try_into().unwrap();

  assert!(ran["command"].as_str().unwrap().contains("export BMO_SECRET_SITE_LOGIN_FILE="));
  assert!(removal["command"].as_str().unwrap().starts_with("rm -rf \"${TMPDIR:-/tmp}/bmo-secret-"));
  assert!(
    answered.text().ends_with(
      "Whether the secrets' folder on pc-four was removed could not be confirmed: its removal ended with exit \
       code 3; the next call handing a file there removes it once a day old."
    ),
    "{}",
    answered.text()
  );
}

#[tokio::test]
async fn the_owner_reads_its_own_account_s_secrets_and_never_the_platform_s() {
  let stood = stocked("ok\n", 0).await;
  let own = json!({ "node": "pc-four", "command": "x", "secrets": [{ "name": "own-token", "as": "env" }] });
  let answered = answer(&handed("pc-agent", &[], own), &stood.ground).await;

  assert!(!answered.is_error, "{}", answered.text());
  assert_eq!(bodies(&stood)[0]["env"], json!({ "BMO_SECRET_OWN_TOKEN": "throwaway-own-5150" }));

  let platform = json!({ "node": "pc-four", "command": "x", "secrets": ["site-login"] });
  let refused = answer(&handed("pc-agent", &[], platform), &stood.ground).await;

  assert!(refused.is_error);
  assert_eq!(refused.text(), "No secret named site-login stands among account 4's secrets.");
  assert_eq!(bodies(&stood).len(), 1);
}

#[tokio::test]
async fn a_name_that_is_not_plain_or_names_nothing_and_a_variable_named_twice_are_refused_before_the_hub() {
  let stood = stocked("ok\n", 0).await;
  let asked = async |secrets: Value, env: Value| {
    let arguments = json!({ "node": "pc-four", "command": "x", "env": env, "secrets": secrets });

    answer(&handed("house", &["node.admin"], arguments), &stood.ground).await
  };

  for name in ["../users/4/own-token", ".hidden", "site-login.previous", "users"] {
    let refused = asked(json!([name]), json!({})).await;

    assert!(refused.is_error && refused.text().contains(name), "{}", refused.text());
  }

  assert_eq!(
    asked(json!(["nothing-here"]), json!({})).await.text(),
    "No secret named nothing-here stands among the platform's secrets."
  );
  assert_eq!(
    asked(json!(["site-login"]), json!({ "PASSWORD": "mine" })).await.text(),
    "The variable PASSWORD is named twice among the call's env and its secrets: give a prefix."
  );
  assert_eq!(
    asked(json!(["site-login", { "name": "site-login", "as": "both" }]), json!({}))
      .await
      .text(),
    "The variable USERNAME is named twice among the call's env and its secrets: give a prefix."
  );
  assert!(bodies(&stood).is_empty());
}

#[tokio::test]
async fn a_stranger_learns_nothing_of_which_secrets_stand() {
  let stood = stocked("ok\n", 0).await;
  let arguments = json!({ "node": "pc-four", "command": "x", "secrets": ["nothing-here"] });
  let refused = answer(&handed("stranger", &[], arguments), &stood.ground).await;

  assert!(refused.is_error);
  assert!(!refused.text().contains("nothing-here"), "{}", refused.text());
}
