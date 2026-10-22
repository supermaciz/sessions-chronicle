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

-- ---------------------------------------------------------------------------
-- ses_v2_tools: tool-call lifecycle. One assistant message carries the full
-- status spread: streaming (raw string input), running, completed with
-- text/file/text result blocks, error with structured message plus partial
-- text, an unknown status, and a reversed (invalid) execution-time range.
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_tools', 'prj_v2', '/tmp/synthetic-project', 'Tool lifecycle', NULL, '0.0.0', 'ses-v2-tools', NULL, 0, 0, 0, 0, 0, 1712000200000, 1712000202000);

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_tools_user', 'ses_v2_tools', 'user', 1, 1712000200000, 1712000200000, '{"text":"run the tools","time":{"created":1712000200000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_tools_asst', 'ses_v2_tools', 'assistant', 2, 1712000201000, 1712000201000, '{"agent":"primary","content":[{"type":"tool","id":"call_stream","name":"bash","state":{"status":"streaming","input":"{\"path\":"},"time":{"created":1712000201000}},{"type":"tool","id":"call_run","name":"bash","state":{"status":"running","input":{"command":"ls"},"metadata":{"note":"x"}},"time":{"created":1712000201100,"ran":1712000201100}},{"type":"tool","id":"call_done","name":"read","state":{"status":"completed","input":{"path":"/tmp/a.txt"},"content":[{"type":"text","text":"first"},{"type":"file","uri":"file:///tmp/a.txt","mime":"text/plain"},{"type":"text","text":"second"}]},"time":{"created":1712000201200,"ran":1712000201200,"completed":1712000201700}},{"type":"tool","id":"call_err","name":"write","state":{"status":"error","input":{"path":"/etc/blocked"},"error":{"type":"permission","message":"permission denied"},"content":[{"type":"text","text":"partial output"}]},"time":{"created":1712000201800}},{"type":"tool","id":"call_unk","name":"grep","state":{"status":"weird","input":{"pattern":"rust"}},"time":{"created":1712000201900}},{"type":"tool","id":"call_inv","name":"glob","state":{"status":"completed","input":{"pattern":"**/*.rs"},"content":[{"type":"text","text":"y"}]},"time":{"created":1712000202000,"ran":1712000202200,"completed":1712000202000}}],"time":{"created":1712000201000}}');

-- ---------------------------------------------------------------------------
-- ses_v2_skill_tool: an ordinary `skill` tool call (stays a tool call) next to
-- injected skill context (user-attached skill stays context).
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_skill_tool', 'prj_v2', '/tmp/synthetic-project', 'Skill tool', NULL, '0.0.0', 'ses-v2-skill-tool', NULL, 0, 0, 0, 0, 0, 1712000400000, 1712000401000);

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_skill_tool_user', 'ses_v2_skill_tool', 'user', 1, 1712000400000, 1712000400000, '{"text":"invoke the skill","skills":[{"id":"skill_a","name":"Skill A","text":"Run the brainstorming workflow before starting creative work."}],"time":{"created":1712000400000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_skill_tool_asst', 'ses_v2_skill_tool', 'assistant', 2, 1712000401000, 1712000401000, '{"agent":"primary","content":[{"type":"tool","id":"call_skill","name":"skill","state":{"status":"completed","input":{"name":"brainstorming"},"metadata":{"name":"brainstorming","dir":"/tmp/skills/brainstorming"}},"time":{"created":1712000401000}},{"type":"text","text":"skill loaded"}],"time":{"created":1712000401000}}');

-- ---------------------------------------------------------------------------
-- ses_v2_task_done: task + synthetic completion. The task carries no result
-- content, so the completion fills the absent result. The completion prose
-- deliberately names a different sessionID than metadata.childID to prove the
-- metadata is authoritative.
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_task_done', 'prj_v2', '/tmp/synthetic-project', 'Task and completion', NULL, '0.0.0', 'ses-v2-task-done', NULL, 0, 0, 0, 0, 0, 1712000300000, 1712000302000);

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_task_done_user', 'ses_v2_task_done', 'user', 1, 1712000300000, 1712000300000, '{"text":"delegate","time":{"created":1712000300000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_task_done_asst', 'ses_v2_task_done', 'assistant', 2, 1712000301000, 1712000301000, '{"agent":"primary","content":[{"type":"tool","id":"call_t1","name":"task","state":{"status":"completed","input":{"description":"Explore docs","prompt":"Find parser files"},"metadata":{"sessionId":"ses_child_a"}},"time":{"created":1712000301000}}],"time":{"created":1712000301000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_task_done_completion', 'ses_v2_task_done', 'synthetic', 3, 1712000302000, 1712000302000, '{"type":"synthetic","text":"<subagent sessionID=\"ses_child_fake\" state=\"completed\">done</subagent>","description":"Explore docs","metadata":{"source":"subagent","childID":"ses_child_a","agent":"explore","state":"completed"},"time":{"created":1712000302000}}');

-- ---------------------------------------------------------------------------
-- ses_v2_completion_only: a subagent completion with no matching task. The
-- standalone subagent row is keyed by the native synthetic message id.
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_completion_only', 'prj_v2', '/tmp/synthetic-project', 'Completion only', NULL, '0.0.0', 'ses-v2-completion-only', NULL, 0, 0, 0, 0, 0, 1712000500000, 1712000501000);

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_completion_user', 'ses_v2_completion_only', 'user', 1, 1712000500000, 1712000500000, '{"text":"nothing here","time":{"created":1712000500000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_completion', 'ses_v2_completion_only', 'synthetic', 2, 1712000501000, 1712000501000, '{"type":"synthetic","text":"<subagent sessionID=\"ses_child_o\" state=\"completed\">orphan</subagent>","description":"Orphan work","metadata":{"source":"subagent","childID":"ses_child_o","agent":"explore","state":"completed"},"time":{"created":1712000501000}}');

-- ---------------------------------------------------------------------------
-- ses_v2_child_missing: completion whose child has no session_v2 row. The
-- native childID is preserved even though the child is not indexed.
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_child_missing', 'prj_v2', '/tmp/synthetic-project', 'Child missing', NULL, '0.0.0', 'ses-v2-child-missing', NULL, 0, 0, 0, 0, 0, 1712000600000, 1712000601000);

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_child_missing_user', 'ses_v2_child_missing', 'user', 1, 1712000600000, 1712000600000, '{"text":"delegate missing","time":{"created":1712000600000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_missing_completion', 'ses_v2_child_missing', 'synthetic', 2, 1712000601000, 1712000601000, '{"type":"synthetic","text":"<subagent sessionID=\"ses_child_missing\" state=\"completed\">gone</subagent>","description":"Missing child","metadata":{"source":"subagent","childID":"ses_child_missing","agent":"explore","state":"completed"},"time":{"created":1712000601000}}');

-- ---------------------------------------------------------------------------
-- ses_v2_resumed: two task invocations that resume the same child, each with
-- its own completion. Explicit task results must survive; completion text must
-- not overwrite them, and the task tool must not also emit an ordinary tool
-- call.
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_resumed', 'prj_v2', '/tmp/synthetic-project', 'Resumed child', NULL, '0.0.0', 'ses-v2-resumed', NULL, 0, 0, 0, 0, 0, 1712000700000, 1712000704000);

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_resumed_user', 'ses_v2_resumed', 'user', 1, 1712000700000, 1712000700000, '{"text":"resume work","time":{"created":1712000700000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_resumed_asst1', 'ses_v2_resumed', 'assistant', 2, 1712000701000, 1712000701000, '{"agent":"primary","content":[{"type":"tool","id":"call_r1","name":"task","state":{"status":"completed","input":{"description":"Explore","prompt":"first prompt"},"metadata":{"sessionId":"ses_child_r"},"content":[{"type":"text","text":"first result"}]},"time":{"created":1712000701000}}],"time":{"created":1712000701000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_resumed_completion1', 'ses_v2_resumed', 'synthetic', 3, 1712000702000, 1712000702000, '{"type":"synthetic","text":"completion one","description":"Explore","metadata":{"source":"subagent","childID":"ses_child_r","agent":"explore","state":"completed"},"time":{"created":1712000702000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_resumed_asst2', 'ses_v2_resumed', 'assistant', 4, 1712000703000, 1712000703000, '{"agent":"primary","content":[{"type":"tool","id":"call_r2","name":"task","state":{"status":"completed","input":{"description":"Explore","prompt":"second prompt"},"metadata":{"sessionId":"ses_child_r"},"content":[{"type":"text","text":"second result"}]},"time":{"created":1712000703000}}],"time":{"created":1712000703000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_resumed_completion2', 'ses_v2_resumed', 'synthetic', 5, 1712000704000, 1712000704000, '{"type":"synthetic","text":"completion two","description":"Explore","metadata":{"source":"subagent","childID":"ses_child_r","agent":"explore","state":"completed"},"time":{"created":1712000704000}}');

-- ---------------------------------------------------------------------------
-- ses_v2_completion_before_task: the completion precedes its task in seq
-- order. Correlation only looks backwards, so the completion stays standalone
-- and the task remains a separate subagent row.
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_completion_before_task', 'prj_v2', '/tmp/synthetic-project', 'Completion before task', NULL, '0.0.0', 'ses-v2-completion-before-task', NULL, 0, 0, 0, 0, 0, 1712000800000, 1712000802000);

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_order_user', 'ses_v2_completion_before_task', 'user', 1, 1712000800000, 1712000800000, '{"text":"order","time":{"created":1712000800000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_early_completion', 'ses_v2_completion_before_task', 'synthetic', 2, 1712000801000, 1712000801000, '{"type":"synthetic","text":"early completion","description":"Explore","metadata":{"source":"subagent","childID":"ses_child_b","agent":"explore","state":"completed"},"time":{"created":1712000801000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_late_task', 'ses_v2_completion_before_task', 'assistant', 3, 1712000802000, 1712000802000, '{"agent":"primary","content":[{"type":"tool","id":"call_b1","name":"task","state":{"status":"completed","input":{"description":"Explore","prompt":"late prompt"},"metadata":{"sessionId":"ses_child_b"},"content":[{"type":"text","text":"late task"}]},"time":{"created":1712000802000}}],"time":{"created":1712000802000}}');

-- ---------------------------------------------------------------------------
-- ses_v2_ambiguous: two task invocations for the same child in one assistant
-- message share a seq, so the following completion cannot be matched uniquely.
-- Neither task result is merged; the completion stays standalone.
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_ambiguous', 'prj_v2', '/tmp/synthetic-project', 'Ambiguous completion', NULL, '0.0.0', 'ses-v2-ambiguous', NULL, 0, 0, 0, 0, 0, 1712000900000, 1712000902000);

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_ambig_user', 'ses_v2_ambiguous', 'user', 1, 1712000900000, 1712000900000, '{"text":"parallel","time":{"created":1712000900000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_ambig_asst', 'ses_v2_ambiguous', 'assistant', 2, 1712000901000, 1712000901000, '{"agent":"primary","content":[{"type":"tool","id":"call_a1","name":"task","state":{"status":"completed","input":{"description":"Explore","prompt":"one"},"metadata":{"sessionId":"ses_child_amb"},"content":[{"type":"text","text":"task one result"}]},"time":{"created":1712000901000}},{"type":"tool","id":"call_a2","name":"task","state":{"status":"completed","input":{"description":"Explore","prompt":"two"},"metadata":{"sessionId":"ses_child_amb"},"content":[{"type":"text","text":"task two result"}]},"time":{"created":1712000901000}}],"time":{"created":1712000901000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_ambig_completion', 'ses_v2_ambiguous', 'synthetic', 3, 1712000902000, 1712000902000, '{"type":"synthetic","text":"ambiguous completion","description":"Explore","metadata":{"source":"subagent","childID":"ses_child_amb","agent":"explore","state":"completed"},"time":{"created":1712000902000}}');

-- ---------------------------------------------------------------------------
-- ses_v2_task_id_link: a task tool that only carries `input.task_id` (a resume
-- request) and no `state.metadata.sessionId`. `input.task_id` alone must not
-- establish a child link, so a completion naming that id stays standalone.
-- ---------------------------------------------------------------------------
INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated)
VALUES ('ses_v2_task_id_link', 'prj_v2', '/tmp/synthetic-project', 'Task id link', NULL, '0.0.0', 'ses-v2-task-id-link', NULL, 0, 0, 0, 0, 0, 1712001000000, 1712001002000);

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_task_id_user', 'ses_v2_task_id_link', 'user', 1, 1712001000000, 1712001000000, '{"text":"resume requested","time":{"created":1712001000000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_task_id_asst', 'ses_v2_task_id_link', 'assistant', 2, 1712001001000, 1712001001000, '{"agent":"primary","content":[{"type":"tool","id":"call_tid","name":"task","state":{"status":"completed","input":{"description":"Resume","prompt":"continue","task_id":"ses_requested"},"content":[{"type":"text","text":"resumed result"}]},"time":{"created":1712001001000}}],"time":{"created":1712001001000}}');

INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
VALUES ('msg_task_id_completion', 'ses_v2_task_id_link', 'synthetic', 3, 1712001002000, 1712001002000, '{"type":"synthetic","text":"resume done","description":"Resume","metadata":{"source":"subagent","childID":"ses_requested","agent":"explore","state":"completed"},"time":{"created":1712001002000}}');
