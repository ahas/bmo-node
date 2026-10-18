//! The tunnel door's wire contract (docs/llm-bridge.md), asked of the command under test as a node's tunnel
//! and a stranger ask it: the door started as a process on a free port over a scratch data root and its
//! list of key files, with a TCP echo standing in for this machine's SSH. The command is the crate's
//! binary, or the one `LLM_BRIDGE_CONTRACT_COMMAND` names.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_KEY: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

/// SSH, standing in: every connection echoed back until it ends, or until it is sent `bye`, and the
/// connections that ended from the far side counted.
struct Echo {
  listen: String,
  ended: Arc<AtomicUsize>,
}

impl Echo {
  async fn start() -> Echo {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listen = listener.local_addr().unwrap().to_string();
    let ended = Arc::new(AtomicUsize::new(0));
    let counting = Arc::clone(&ended);
    tokio::spawn(async move {
      while let Ok((mut stream, _)) = listener.accept().await {
        let counting = Arc::clone(&counting);
        tokio::spawn(async move {
          let mut buffer = vec![0u8; 64 * 1024];
          loop {
            match stream.read(&mut buffer).await {
              Ok(0) | Err(_) => {
                counting.fetch_add(1, Ordering::SeqCst);
                return;
              }
              Ok(read) if &buffer[..read] == b"bye" => return,
              Ok(read) => {
                if stream.write_all(&buffer[..read]).await.is_err() {
                  return;
                }
              }
            }
          }
        });
      }
    });
    Echo { listen, ended }
  }

  fn ended(&self) -> usize {
    self.ended.load(Ordering::SeqCst)
  }
}

fn command() -> Command {
  match std::env::var("LLM_BRIDGE_CONTRACT_COMMAND") {
    Ok(named) if !named.trim().is_empty() => {
      let mut words = named.split_whitespace();
      let mut command = Command::new(words.next().unwrap());
      command.args(words);
      command
    }
    _ => Command::new(crate::binary::built("llm-bridge", "llm-bridge")),
  }
}

struct Door {
  dir: tempfile::TempDir,
  child: Child,
  port: u16,
}

impl Door {
  /// The door started with its list naming the key files given, piping to the target given.
  async fn start(target: &str, listed: &[PathBuf]) -> Door {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data/env")).unwrap();
    let log = std::fs::File::create(dir.path().join("door.log")).unwrap();
    // A port of its own choosing, which it names as it starts listening, so no other process can
    // take the port between its choosing and its binding.
    let child = command()
      .env("BMO_DATA_DIR", dir.path().join("data"))
      .env("LLM_BRIDGE_LISTEN", "127.0.0.1:0")
      .env("LLM_BRIDGE_TARGET", target)
      .stdin(Stdio::null())
      .stdout(log)
      .stderr(Stdio::null())
      .process_group(0)
      .spawn()
      .expect("the door starts");
    let mut door = Door {
      dir,
      child,
      port: 0,
    };
    door.list(listed);
    let ready = Instant::now() + Duration::from_secs(20);
    // The door's own line: `llm-bridge: listening on 127.0.0.1:<port>`.
    door.port = loop {
      let said = std::fs::read_to_string(door.dir.path().join("door.log")).unwrap_or_default();
      let heard = said.lines().find_map(|line| {
        line.strip_prefix("llm-bridge: listening on 127.0.0.1:")?.trim().parse::<u16>().ok()
      });
      if let Some(port) = heard {
        break port;
      }
      assert!(Instant::now() < ready, "the door never listened; it said {said}");
      tokio::time::sleep(Duration::from_millis(20)).await;
    };
    door
  }

  fn list(&self, listed: &[PathBuf]) {
    let lines: Vec<String> = listed.iter().map(|path| format!("{}\n", path.display())).collect();
    std::fs::write(self.dir.path().join("data/env/tunnel-door.keys"), lines.concat()).unwrap();
  }

  fn unlist(&self) {
    std::fs::remove_file(self.dir.path().join("data/env/tunnel-door.keys")).unwrap();
  }

  /// A key file of the scratch tree, written with the mode given.
  fn key_file(&self, name: &str, mode: u32, text: &str) -> PathBuf {
    key_file(self.dir.path(), name, mode, text)
  }

  /// A plain request, its status and its body.
  async fn ask(&self, authorization: Option<&str>) -> (u16, String) {
    let stream = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
    let (mut sender, connection) =
      hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.unwrap();
    tokio::spawn(connection);
    let mut request = hyper::Request::get("/").header("host", "llm-tunnel.example.com");
    if let Some(authorization) = authorization {
      request = request.header("authorization", authorization);
    }
    let answer = sender.send_request(request.body(Empty::<Bytes>::new()).unwrap()).await.unwrap();
    let status = answer.status().as_u16();
    let body = answer.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
  }

  async fn open(
    &self,
    authorization: &str,
  ) -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>, WsError> {
    let mut request = format!("ws://127.0.0.1:{}/", self.port).into_client_request().unwrap();
    request.headers_mut().insert("authorization", authorization.parse().unwrap());
    tokio_tungstenite::connect_async(request).await.map(|(socket, _)| socket)
  }
}

impl Drop for Door {
  fn drop(&mut self) {
    unsafe {
      libc::kill(-(self.child.id() as i32), libc::SIGKILL);
    }
    let _ = self.child.wait();
  }
}

fn key_file(dir: &Path, name: &str, mode: u32, text: &str) -> PathBuf {
  let path = dir.join(name);
  std::fs::write(&path, format!("{text}\n")).unwrap();
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
  path
}

fn bearer(key: &str) -> String {
  format!("Bearer {key}")
}

async fn until(what: &str, holds: impl Fn() -> bool) {
  let bound = Instant::now() + Duration::from_secs(15);
  while !holds() {
    assert!(Instant::now() < bound, "waited in vain for {what}");
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
}

/// What comes back from the far side until `wanted` bytes have, within a bound.
async fn heard(
  socket: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
  wanted: usize,
) -> Vec<u8> {
  let mut got = Vec::new();
  while got.len() < wanted {
    let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    if let Message::Binary(bytes) = message {
      got.extend_from_slice(&bytes);
    }
  }
  got
}

/// Whether the far side closed the WebSocket within a bound.
async fn closed(
  socket: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
) -> bool {
  let bound = Instant::now() + Duration::from_secs(10);
  while Instant::now() < bound {
    match tokio::time::timeout(Duration::from_secs(10), socket.next()).await {
      Ok(None | Some(Err(_)) | Some(Ok(Message::Close(_)))) => return true,
      Ok(Some(Ok(_))) => continue,
      Err(_) => return false,
    }
  }
  false
}

#[tokio::test]
async fn a_request_without_the_key_or_with_a_wrong_one_is_refused_saying_nothing_more() {
  let echo = Echo::start().await;
  let door = Door::start(&echo.listen, &[]).await;
  door.list(&[door.key_file("first.token", 0o600, KEY)]);

  for authorization in [None, Some(bearer(&"b".repeat(64))), Some(KEY.to_owned()), Some(bearer(&KEY[1..]))]
  {
    assert_eq!(door.ask(authorization.as_deref()).await, (401, "unauthorized\n".to_owned()));
  }
  match door.open(&bearer(&"b".repeat(64))).await {
    Err(WsError::Http(answer)) => assert_eq!(answer.status().as_u16(), 401),
    other => panic!("a WebSocket with a wrong key was not refused: {:?}", other.map(|_| ())),
  }
}

#[tokio::test]
async fn a_key_whose_file_is_loose_or_empty_or_a_missing_list_lets_nobody_in() {
  let echo = Echo::start().await;
  let door = Door::start(&echo.listen, &[]).await;

  door.list(&[door.key_file("first.token", 0o644, KEY)]);
  assert_eq!(door.ask(Some(&bearer(KEY))).await.0, 401);
  door.list(&[door.key_file("empty.token", 0o600, "")]);
  assert_eq!(door.ask(Some("Bearer ")).await.0, 401);
  door.unlist();
  assert_eq!(door.ask(Some(&bearer(KEY))).await.0, 401);
}

#[tokio::test]
async fn any_listed_services_key_on_a_request_that_is_no_websocket_is_asked_to_upgrade() {
  let echo = Echo::start().await;
  let door = Door::start(&echo.listen, &[]).await;
  door.list(&[
    door.key_file("first.token", 0o600, KEY),
    door.key_file("second.token", 0o600, OTHER_KEY),
  ]);

  for key in [KEY, OTHER_KEY] {
    assert_eq!(door.ask(Some(&bearer(key))).await, (426, "upgrade required\n".to_owned()));
  }
}

#[tokio::test]
async fn the_list_is_read_again_for_every_connection() {
  let echo = Echo::start().await;
  let door = Door::start(&echo.listen, &[]).await;
  assert_eq!(door.ask(Some(&bearer(KEY))).await.0, 401);

  door.list(&[door.key_file("first.token", 0o600, KEY)]);
  assert_eq!(door.ask(Some(&bearer(KEY))).await.0, 426);

  door.list(&[]);
  assert_eq!(door.ask(Some(&bearer(KEY))).await.0, 401);
}

#[tokio::test]
async fn a_taken_websocket_carries_bytes_both_ways_to_its_target() {
  let echo = Echo::start().await;
  let door = Door::start(&echo.listen, &[]).await;
  door.list(&[door.key_file("first.token", 0o600, KEY)]);

  let mut socket = door.open(&bearer(KEY)).await.unwrap();
  socket.send(Message::binary(&b"SSH-2.0-check\r\n"[..])).await.unwrap();
  assert_eq!(heard(&mut socket, 15).await, b"SSH-2.0-check\r\n");
  socket.send(Message::binary(vec![7u8; 100_000])).await.unwrap();
  assert_eq!(heard(&mut socket, 100_000).await, vec![7u8; 100_000]);
}

#[tokio::test]
async fn bytes_sent_the_moment_the_websocket_opens_reach_the_target_in_order() {
  let echo = Echo::start().await;
  let door = Door::start(&echo.listen, &[]).await;
  door.list(&[door.key_file("first.token", 0o600, KEY)]);

  let mut socket = door.open(&bearer(KEY)).await.unwrap();
  for chunk in ["one,", "two,", "three"] {
    socket.send(Message::binary(chunk.as_bytes().to_vec())).await.unwrap();
  }
  assert_eq!(heard(&mut socket, 13).await, b"one,two,three");
}

#[tokio::test]
async fn a_websocket_is_closed_where_its_target_cannot_be_reached_or_ends() {
  // A target with nothing listening: its port held by a socket that is bound and never listens, so
  // a connection to it is refused and no other process takes it.
  let held = tokio::net::TcpSocket::new_v4().unwrap();
  held.bind("127.0.0.1:0".parse().unwrap()).unwrap();
  let gone = held.local_addr().unwrap().to_string();
  let door = Door::start(&gone, &[]).await;
  door.list(&[door.key_file("first.token", 0o600, KEY)]);
  let mut socket = door.open(&bearer(KEY)).await.unwrap();
  assert!(closed(&mut socket).await, "the WebSocket stayed open with no target");

  let echo = Echo::start().await;
  let door = Door::start(&echo.listen, &[]).await;
  door.list(&[door.key_file("first.token", 0o600, KEY)]);
  let mut socket = door.open(&bearer(KEY)).await.unwrap();
  socket.send(Message::binary(&b"bye"[..])).await.unwrap();
  assert!(closed(&mut socket).await, "the WebSocket stayed open after its target ended");
}

#[tokio::test]
async fn the_targets_connection_ends_when_the_websocket_closes() {
  let echo = Echo::start().await;
  let door = Door::start(&echo.listen, &[]).await;
  door.list(&[door.key_file("first.token", 0o600, KEY)]);

  let mut socket = door.open(&bearer(KEY)).await.unwrap();
  socket.send(Message::binary(&b"hello"[..])).await.unwrap();
  assert_eq!(heard(&mut socket, 5).await, b"hello");
  socket.close(None).await.unwrap();

  until("the target's connection to end", || echo.ended() == 1).await;
}
