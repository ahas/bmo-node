//! How the service stops (docs/api.md, 「Stopping」): told by a signal, it stops accepting, ends its event
//! streams and gives the calls under way a bounded time to finish.

use std::future::{Future, IntoFuture};
use std::io;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

use crate::{Service, app};

/// How long the calls under way are given to finish once the service is told to stop.
pub const DRAIN: Duration = Duration::from_secs(30);

/// How a drain ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drained {
  /// Every connection closed within the time given.
  Finished,
  /// The time given ran out with a call still under way, which ends with the process.
  Cut,
}

/// A future that ends once the process is told to stop: SIGTERM, as systemd stops the unit, or SIGINT
/// from a terminal. Both are taken from the moment this returns, so neither ends the process unhandled.
pub fn told_to_stop() -> io::Result<impl Future<Output = ()> + Send + 'static> {
  let mut terminate = signal(SignalKind::terminate())?;
  let mut interrupt = signal(SignalKind::interrupt())?;

  Ok(async move {
    tokio::select! {
      _ = terminate.recv() => {}
      _ = interrupt.recv() => {}
    }
  })
}

/// The service's routes served on `listener` until `stop` ends, then drained: the listener closed, the
/// event streams ended, an idle connection closed, and a call under way given `drain` to finish.
pub async fn serve(
  listener: TcpListener,
  service: Arc<Service>,
  stop: impl Future<Output = ()>,
  drain: Duration,
) -> io::Result<Drained> {
  let stopping = service.stopping.clone();
  let draining = stopping.clone();
  let served = axum::serve(listener, app(service))
    .with_graceful_shutdown(async move { draining.cancelled().await })
    .into_future();
  let mut served = pin!(served);

  tokio::select! {
    ended = &mut served => return ended.map(|()| Drained::Finished),
    () = stop => stopping.cancel(),
  }

  match tokio::time::timeout(drain, served).await {
    Ok(ended) => ended.map(|()| Drained::Finished),
    Err(_) => Ok(Drained::Cut),
  }
}
