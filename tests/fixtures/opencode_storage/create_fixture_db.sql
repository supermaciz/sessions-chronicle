CREATE TABLE IF NOT EXISTS project (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS session (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    parent_id TEXT,
    slug TEXT NOT NULL,
    directory TEXT NOT NULL,
    title TEXT NOT NULL,
    version TEXT NOT NULL,
    share_url TEXT,
    summary_additions INTEGER,
    summary_deletions INTEGER,
    summary_files INTEGER,
    summary_diffs TEXT,
    revert TEXT,
    permission TEXT,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_compacting INTEGER,
    time_archived INTEGER
);

CREATE TABLE IF NOT EXISTS message (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS part (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data TEXT NOT NULL
);

INSERT INTO project (id, name) VALUES ('proj-a', 'Alpha Project');

INSERT INTO session (id, project_id, parent_id, slug, directory, title, version,
                     time_created, time_updated)
VALUES ('session-001', 'proj-a', NULL, 'session-001', '/projects/alpha',
        'Updated title from SQLite', 'v1', 1704067200000, 1704067260000);

INSERT INTO message (id, session_id, time_created, time_updated, data)
VALUES ('msg-sqlite-001', 'session-001', 1704067200000, 1704067200000,
        '{"role": "user"}');

INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
VALUES ('part-sqlite-001', 'msg-sqlite-001', 'session-001', 1704067200000, 1704067200000,
        '{"type": "text", "text": "Hello from SQLite"}');

INSERT INTO message (id, session_id, time_created, time_updated, data)
VALUES ('msg-sqlite-002', 'session-001', 1704067210000, 1704067210000,
        '{"role": "assistant"}');

INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
VALUES ('part-sqlite-002', 'msg-sqlite-002', 'session-001', 1704067210000, 1704067210000,
        '{"type": "text", "text": "SQLite assistant response"}');

INSERT INTO session (id, project_id, parent_id, slug, directory, title, version,
                     time_created, time_updated)
VALUES ('session-sqlite-only', 'proj-a', NULL, 'sqlite-only', '/projects/beta',
        'SQLite-only session', 'v1', 1704070000000, 1704070060000);

INSERT INTO message (id, session_id, time_created, time_updated, data)
VALUES ('msg-sqlite-only-001', 'session-sqlite-only', 1704070000000, 1704070000000,
        '{"role": "user"}');

INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
VALUES ('part-sqlite-only-001', 'msg-sqlite-only-001', 'session-sqlite-only',
        1704070000000, 1704070000000,
        '{"type": "text", "text": "This session only exists in SQLite"}');

INSERT INTO message (id, session_id, time_created, time_updated, data)
VALUES ('msg-sqlite-only-002', 'session-sqlite-only', 1704070010000, 1704070010000,
        '{"role": "assistant"}');

INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
VALUES ('part-sqlite-only-002', 'msg-sqlite-only-002', 'session-sqlite-only',
        1704070010000, 1704070010000,
        '{"type": "text", "text": "Response from SQLite-only session"}');

INSERT INTO session (id, project_id, parent_id, slug, directory, title, version,
                     time_created, time_updated)
VALUES ('session-sqlite-subagent', 'proj-a', 'session-sqlite-only', 'sqlite-sub',
        '/projects/beta', 'SQLite subagent', 'v1', 1704070020000, 1704070030000);

INSERT INTO message (id, session_id, time_created, time_updated, data)
VALUES ('msg-sqlite-sub-001', 'session-sqlite-subagent', 1704070020000, 1704070020000,
        '{"role": "user"}');

INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
VALUES ('part-sqlite-sub-001', 'msg-sqlite-sub-001', 'session-sqlite-subagent',
        1704070020000, 1704070020000,
        '{"type": "text", "text": "Subagent task from SQLite"}');

INSERT INTO message (id, session_id, time_created, time_updated, data)
VALUES ('msg-sqlite-sub-002', 'session-sqlite-subagent', 1704070025000, 1704070025000,
        '{"role": "assistant"}');

INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
VALUES ('part-sqlite-sub-002', 'msg-sqlite-sub-002', 'session-sqlite-subagent',
        1704070025000, 1704070025000,
        '{"type": "text", "text": "Subagent response from SQLite"}');
