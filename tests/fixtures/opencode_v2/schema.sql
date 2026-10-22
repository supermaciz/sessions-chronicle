-- Synthetic, source-derived schema for the OpenCode V2 native reader.
-- Derived from upstream sst/opencode (anomalyco/opencode) commit
-- 0c6be8694894dc96b87c5a7b773d52e57ddaa1b2:
--   packages/core/src/session/sql.ts
--   packages/core/src/database/schema.sql.ts (Timestamps)
-- Only the columns the reader consumes (plus a few NOT NULL upstream columns)
-- are persisted. See README.md for full provenance and omissions.

CREATE TABLE session_v2 (
    id                   TEXT PRIMARY KEY,
    project_id           TEXT NOT NULL,
    directory            TEXT NOT NULL,
    title                TEXT,
    parent_id            TEXT,
    version              TEXT NOT NULL,
    slug                 TEXT NOT NULL,
    model                TEXT,
    tokens_input         INTEGER NOT NULL DEFAULT 0,
    tokens_output        INTEGER NOT NULL DEFAULT 0,
    tokens_reasoning     INTEGER NOT NULL DEFAULT 0,
    tokens_cache_read    INTEGER NOT NULL DEFAULT 0,
    tokens_cache_write   INTEGER NOT NULL DEFAULT 0,
    time_created         INTEGER NOT NULL,
    time_updated         INTEGER NOT NULL
);

CREATE TABLE session_message (
    id             TEXT PRIMARY KEY,
    session_id     TEXT NOT NULL,
    type           TEXT NOT NULL,
    seq            INTEGER NOT NULL,
    time_created   INTEGER NOT NULL,
    time_updated   INTEGER NOT NULL,
    data           TEXT NOT NULL,
    UNIQUE (session_id, seq)
);
