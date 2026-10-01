# Queue audit log

Every change to a guild's queue is written to `queue_audit`: who (`actor_user_id`,
NULL for the bot), how (`source`: slash, prefix, web, bot; `command`), where
(`voice_channel_id`, `origin_channel_id`), when (`at`), and what (`action`, with
the details in `detail` as JSON). Design:
`docs/superpowers/specs/2026-09-30-queue-audit-log-design.md`.

Not recorded: a track ending on its own (see `play_log`); refused attempts; volume,
seek, repeat and the autoplay settings. Without a database, events go to the bot's
log at `info` as `queue audit (no database)`. A full writer channel drops an event
with a `warn`; commands never wait on the log.

⚠️ `/gp` rows (`command = 'gp'`) carry the round's song titles. Any reader shown to
players must hide a guild's game rows while its game is running.

## Example queries

    -- The last 20 changes in a guild
    SELECT at, actor_user_id, source, command, action, detail
    FROM queue_audit WHERE guild_id = $1 ORDER BY at DESC LIMIT 20;

    -- Who has been moving tracks from the web, this week
    SELECT actor_user_id, count(*) FROM queue_audit
    WHERE source = 'web' AND at > now() - interval '7 days'
    GROUP BY 1 ORDER BY 2 DESC;

    -- Everything the bot did on its own in a guild today
    SELECT at, command, action, detail FROM queue_audit
    WHERE guild_id = $1 AND source = 'bot' AND at > now() - interval '1 day'
    ORDER BY at;
