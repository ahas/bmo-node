//! The door itself: a request refused with no more than its status unless its bearer is a listed key,
//! and a WebSocket that has one piped both ways to the target, this machine's own SSH.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::keys::{bearer_matches, read_door_keys};

/// Where the proxy forwards the tunnel's name to, and what the door pipes to.
pub const DOOR_LISTEN: &str = "0.0.0.0:18022";
pub const SSH_TARGET: &str = "127.0.0.1:22";

pub struct Door {
  /// The door's list: one key file's path to a line.
  pub list: PathBuf,
  pub target: String,
}

// A refusal says nothing about why, and repeats nothing of the request.
fn refusal(status: StatusCode, said: &'static str) -> Response {
  (status, [(header::CONTENT_TYPE, "text/plain;charset=utf-8")], said).into_response()
}

async fn enter(
  State(door): State<Arc<Door>>,
  upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
  request: Request,
) -> Response {
  let bearer = request.headers().get(header::AUTHORIZATION).map(|value| value.as_bytes());
  if !read_door_keys(&door.list).iter().any(|key| bearer_matches(bearer, key)) {
    return refusal(StatusCode::UNAUTHORIZED, "unauthorized\n");
  }
  match upgrade {
    Ok(upgrade) => upgrade.on_upgrade(move |socket| pipe(socket, door.target.clone())),
    Err(_) => refusal(StatusCode::UPGRADE_REQUIRED, "upgrade required\n"),
  }
}

/// The WebSocket and the target joined until either ends. What the WebSocket sends before the target's
/// connection stands waits in it unread, and is carried once the connection opens.
async fn pipe(mut socket: WebSocket, target: String) {
  let Ok(stream) = TcpStream::connect(&target).await else {
    let _ = socket.close().await;
    return;
  };
  let (mut reading, mut writing) = stream.into_split();
  let (mut out, mut inn) = socket.split();

  let upstream = async {
    while let Some(Ok(message)) = inn.next().await {
      let chunk: Bytes = match message {
        Message::Binary(bytes) => bytes,
        Message::Text(text) => Bytes::copy_from_slice(text.as_str().as_bytes()),
        Message::Close(_) => break,
        Message::Ping(_) | Message::Pong(_) => continue,
      };
      if writing.write_all(&chunk).await.is_err() {
        break;
      }
    }
    let _ = writing.shutdown().await;
  };
  let downstream = async {
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
      match reading.read(&mut buffer).await {
        Ok(0) | Err(_) => break,
        Ok(read) => {
          if out.send(Message::Binary(Bytes::copy_from_slice(&buffer[..read]))).await.is_err() {
            return;
          }
        }
      }
    }
    let _ = out.send(Message::Close(None)).await;
  };
  tokio::select! {
    _ = upstream => {}
    _ = downstream => {}
  }
}

/// Every path and method comes to the door.
pub fn router(door: Arc<Door>) -> Router {
  Router::new().fallback(enter).with_state(door)
}
