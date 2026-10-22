-- Synthetic sessions for the OpenCode V2 reader. No personal content.
-- In `ses_v2`, the SQL insertion order below and the `time_created` values
-- deliberately disagree with `seq`: the reader must order by `seq, id`, never
-- by insertion order or timestamp.

-- ---------------------------------------------------------------------------
-- session_v2 rows
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2', 'prj_v2', '/tmp/synthetic-project', '   ', NULL, '0.0.0', 'ses-v2', '{"id":"session/model","providerID":"session"}', 999, 999, 999, 999, 999, 1712000000000, 1712000120000);

INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_system_only', 'prj_v2', '/tmp/synthetic-project', NULL, NULL, '0.0.0', 'ses-v2-system-only', NULL, 0, 0, 0, 0, 0, 1712000000000, 1712000010000);

INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_malformed', 'prj_v2', '/tmp/synthetic-project', NULL, NULL, '0.0.0', 'ses-v2-malformed', NULL, 0, 0, 0, 0, 0, 1712000000000, 1712000020000);

INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_corrupt_user', 'prj_v2', '/tmp/synthetic-project', NULL, NULL, '0.0.0', 'ses-v2-corrupt-user', NULL, 0, 0, 0, 0, 0, 1712000000000, 1712000010000);

INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_malformed_then_valid', 'prj_v2', '/tmp/synthetic-project', NULL, NULL, '0.0.0', 'ses-v2-malformed-then-valid', NULL, 0, 0, 0, 0, 0, 1712000000000, 1712000020000);

INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_null_title', 'prj_v2', '/tmp/synthetic-project', NULL, NULL, '0.0.0', 'ses-v2-null-title', NULL, 0, 0, 0, 0, 0, 1712000000000, 1712000010000);

-- ---------------------------------------------------------------------------
-- ses_v2 messages: inserted out of seq order, timestamps scrambled.
-- ---------------------------------------------------------------------------
INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_comp_01', 'ses_v2', 'compaction', 7, 1712000060000, 1712000060000, '{"status":"completed","reason":"manual","summary":"compaction summary text","time":{"created":1712000060000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_user_real', 'ses_v2', 'user', 2, 1712000010000, 1712000010000, '{"text":"real prompt","time":{"created":1712000010000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_comp_fail', 'ses_v2', 'compaction', 9, 1712000040000, 1712000040000, '{"status":"failed","reason":"auto","error":{"type":"provider_error","message":"compaction provider failed","status":500},"time":{"created":1712000040000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_sys_01', 'ses_v2', 'system', 1, 1712000090000, 1712000090000, '{"text":"system prompt with contextonlyneedle marker","description":"System prompt","time":{"created":1712000090000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_asst_unknown', 'ses_v2', 'assistant', 11, 1712000110000, 1712000110000, '{"agent":"primary","content":[{"type":"future-content","payload":"unknown"}],"time":{"created":1712000110000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_user_skill', 'ses_v2', 'user', 4, 1712000020000, 1712000020000, '{"text":"prompt with skill","skills":[{"id":"skill_a","name":"Skill A","text":"Run the brainstorming workflow before starting creative work."},{"id":"skill_b","name":"Skill B"}],"time":{"created":1712000020000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_skill_01', 'ses_v2', 'skill', 5, 1712000070000, 1712000070000, '{"skill":"skill_a","name":"Skill A","text":"Run the brainstorming workflow before starting creative work.","time":{"created":1712000070000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_comp_run', 'ses_v2', 'compaction', 8, 1712000050000, 1712000050000, '{"status":"running","reason":"auto","summary":"running summary ignored","time":{"created":1712000050000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_final', 'ses_v2', 'assistant', 6, 1712000030000, 1712000030000, '{"agent":"primary","content":[{"type":"text","text":"final answer"}],"tokens":{"input":20,"output":4,"reasoning":0,"cache":{"read":0,"write":0}},"time":{"created":1712000030000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_asst_reason', 'ses_v2', 'assistant', 3, 1712000080000, 1712000080000, '{"agent":"primary","model":{"id":"test/model-a","providerID":"test"},"content":[{"type":"text","text":"answer part one"},{"type":"reasoning","text":"thinking out loud"},{"type":"text","text":"answer part two"}],"tokens":{"input":10,"output":3,"reasoning":0,"cache":{"read":0,"write":0}},"time":{"created":1712000080000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_idle', 'ses_v2', 'idle', 10, 1712000100000, 1712000100000, '{"outcome":"succeeded","time":{"created":1712000100000}}');

-- ---------------------------------------------------------------------------
-- Other sessions
-- ---------------------------------------------------------------------------
INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_sys_only', 'ses_v2_system_only', 'system', 1, 1712000010000, 1712000010000, '{"text":"system only session","time":{"created":1712000010000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_bad', 'ses_v2_malformed', 'assistant', 1, 1712000010000, 1712000010000, '{not valid json');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_malformed_user', 'ses_v2_malformed', 'user', 2, 1712000010000, 1712000010000, '{"text":"first real prompt","time":{"created":1712000010000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_malformed_asst', 'ses_v2_malformed', 'assistant', 3, 1712000020000, 1712000020000, '{"model":{"id":"test/m","providerID":"test"},"content":[{"type":"text","text":"assistant reply"}],"time":{"created":1712000020000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_corrupt', 'ses_v2_corrupt_user', 'user', 1, 1712000010000, 1712000010000, '{broken user json');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_corrupt_first', 'ses_v2_malformed_then_valid', 'user', 1, 1712000010000, 1712000010000, '{broken user json');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_valid_after', 'ses_v2_malformed_then_valid', 'user', 2, 1712000020000, 1712000020000, '{"text":"valid prompt","time":{"created":1712000020000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_null_title_user', 'ses_v2_null_title', 'user', 1, 1712000010000, 1712000010000, '{"text":"null title prompt","time":{"created":1712000010000}}');
