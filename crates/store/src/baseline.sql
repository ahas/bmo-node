-- The store's schema at 1.0.0, whole (docs/store.md, 「The tables」).

CREATE TABLE nodes (
  name TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  kind TEXT NOT NULL,
  host TEXT,
  port INTEGER,
  user TEXT,
  key_path TEXT,
  shell TEXT,
  agent TEXT,
  hub_url TEXT,
  role TEXT,
  position INTEGER NOT NULL,
  state TEXT NOT NULL DEFAULT 'unknown',
  detail TEXT,
  checked_at INTEGER,
  connected_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
, callers TEXT, ownership TEXT NOT NULL DEFAULT 'shared', account TEXT);

CREATE TABLE hardware_samples (
  machine TEXT NOT NULL,
  at INTEGER NOT NULL,
  sample TEXT NOT NULL,
  PRIMARY KEY (machine, at)
);

CREATE INDEX hardware_samples_at ON hardware_samples (at);

CREATE TABLE hardware_rollups (
  machine TEXT NOT NULL,
  at INTEGER NOT NULL,
  tally TEXT NOT NULL,
  PRIMARY KEY (machine, at)
);

CREATE INDEX hardware_rollups_at ON hardware_rollups (at);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE "hubs" (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  url TEXT NOT NULL UNIQUE,
  token_file TEXT NOT NULL,
  label TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE memory_samples (
  machine TEXT NOT NULL,
  at INTEGER NOT NULL,
  sample TEXT NOT NULL,
  PRIMARY KEY (machine, at)
);

CREATE INDEX memory_samples_at ON memory_samples (at);

