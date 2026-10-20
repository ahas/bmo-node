//! The events the service publishes (docs/api.md, 「Events」): each numbered as it is published, kept in a
//! window of the last ones for a reader that resumes, and handed to every stream open at that moment.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use bmo_node_client::events::{Change, Event, EventKind};
use tokio::sync::broadcast;

/// How many past events the service keeps for a reader that resumes.
pub const WINDOW: usize = 4096;

// How many events a slow stream may fall behind before it skips the oldest.
const BEHIND_MOST: usize = 1024;

// How far apart two runs' first numbers stand per millisecond between their starts, so a number an earlier
// run gave is always below the window of a later one.
const NUMBERS_PER_MS: i64 = 1_000;

/// One event as published, with the machine it belongs to, which a stream narrows by.
#[derive(Clone, Debug)]
pub struct Published {
  pub event: Event,
  pub machine_name: Option<String>,
}

impl Published {
  /// Whether a stream narrowed to these kinds and this machine carries the event.
  pub fn carried(&self, kinds: &[EventKind], machine_name: Option<&str>) -> bool {
    (kinds.is_empty() || kinds.contains(&self.event.kind))
      && machine_name.is_none_or(|wanted| self.machine_name.as_deref() == Some(wanted))
  }
}

/// A resume from before the window: the reader reads the collections again.
#[derive(Debug, PartialEq, Eq)]
pub struct Gone;

struct Window {
  next: i64,
  kept: VecDeque<Published>,
}

impl Window {
  /// The number before the oldest event a reader may resume from.
  fn floor(&self) -> i64 {
    self.kept.front().map_or(self.next, |oldest| oldest.event.id) - 1
  }

  fn after(&self, after: i64) -> Result<Vec<Published>, Gone> {
    if after < self.floor() {
      return Err(Gone);
    }

    Ok(self.kept.iter().filter(|kept| kept.event.id > after).cloned().collect())
  }
}

#[derive(Clone)]
pub struct Events {
  window: Arc<Mutex<Window>>,
  sender: broadcast::Sender<Published>,
}

impl Default for Events {
  fn default() -> Self {
    Self::numbered_from(::hardware::now_ms().saturating_mul(NUMBERS_PER_MS))
  }
}

impl Events {
  /// The events of a run whose first event takes the number given.
  pub fn numbered_from(first: i64) -> Self {
    Self {
      window: Arc::new(Mutex::new(Window {
        next: first,
        kept: VecDeque::new(),
      })),
      sender: broadcast::channel(BEHIND_MOST).0,
    }
  }

  fn window(&self) -> std::sync::MutexGuard<'_, Window> {
    self.window.lock().unwrap_or_else(PoisonError::into_inner)
  }

  /// A change published: numbered, kept in the window and handed to every stream open now.
  pub fn publish(&self, change: Change) {
    let mut window = self.window();
    let published = Published {
      machine_name: change.machine_name().map(str::to_owned),
      event: Event {
        id: window.next,
        kind: change.kind(),
        at: ::hardware::now_ms(),
        data: change.data(),
      },
    };

    window.next += 1;
    window.kept.push_back(published.clone());

    while window.kept.len() > WINDOW {
      window.kept.pop_front();
    }

    let _ = self.sender.send(published);
  }

  /// The events kept after the number given, or `Gone` where it stands before the window.
  pub fn after(&self, after: i64) -> Result<Vec<Published>, Gone> {
    self.window().after(after)
  }

  /// The number before the oldest event kept, which a reader asking for everything kept starts after.
  pub fn floor(&self) -> i64 {
    self.window().floor()
  }

  /// A stream of every event published from now on, with the kept events after `after` that come before
  /// it, where a resume is asked; taken together, so none falls between the two.
  pub fn subscribe(
    &self,
    after: Option<i64>,
  ) -> Result<(Vec<Published>, broadcast::Receiver<Published>), Gone> {
    let window = self.window();
    let backlog = match after {
      Some(after) => window.after(after)?,
      None => Vec::new(),
    };

    Ok((backlog, self.sender.subscribe()))
  }
}
