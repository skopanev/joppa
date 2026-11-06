-- Agent tokens are gone: an agent writes under the OAuth session of a person with its alias
-- (049_actor_alias.sql). The tables of the tokens and their revocations leave.
DROP TABLE IF EXISTS joppa.agent_token_revocations;
DROP TABLE IF EXISTS joppa.agent_tokens;
