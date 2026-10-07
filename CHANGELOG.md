# Change Log

## Unreleased

### Security

- **A YouTube playlist link could make yt-dlp read files and post them to the
  channel.** The `list=` value of a `www.youtube.com` link went to yt-dlp as its
  first argument unchecked, and songbird puts no `--` before it, so
  `/optplay mode:all` with `list=--batch-file%3D/proc/self/environ` had yt-dlp
  read the bot's environment and echo every line back as "not a valid URL" --
  and the error reply posted yt-dlp's stderr, `DISCORD_TOKEN` and
  `DATABASE_URL` included, for any member to read. A playlist id must now look
  like one, and a playlist link is rebuilt from it before anything fetches it;
  `yt-dlp` gets `--` before the URL where the bot runs it itself; and no
  subprocess output reaches a user-facing error any more -- it is logged, and
  the reply says the audio couldn't be loaded. **If you run a public instance,
  rotate the bot token and the database password.**
- **Links could point the bot at private addresses.** A link to any site
  without special handling goes to yt-dlp's generic extractor, and
  `/playytplaylist` fetched whatever it was given with no timeout. Links whose
  host is, or resolves to, a loopback, private, link-local, CGNAT or other
  non-public address (`127.0.0.1` where sleevenote listens, the LAN, cloud
  metadata) are now refused, and `/playytplaylist` fetches only a validated
  YouTube playlist link, with timeouts. yt-dlp still follows redirects itself,
  so a public page redirecting inward is not covered.
- **Nothing but an http(s) link reaches yt-dlp as a link any more.** songbird's
  `YoutubeDl::new` passes its string to yt-dlp with no `--` in front, which is
  how the playlist id above became an option. Every link now goes through
  `crack_types::ytdl_for_url`, which accepts only http(s) URLs, and clippy
  refuses `YoutubeDl::new` everywhere else. Search text goes through
  `YoutubeDl::new_search` instead of six hand-built `ytsearch:` strings.
- **crack-voting no longer starts without a real webhook secret.** It fell back
  to `test_secret`, and the compose file defaulted to the same value on a port
  published to every interface, so anyone could post votes for any user.
  `WEBHOOK_SECRET` unset, blank, `test_secret` or `XXXXXX` now stops it at
  startup. The secret is compared in constant time, the `authorization` header
  is no longer printed with the rest of every request's headers, a header that
  is not ASCII no longer panics the request, and bodies over 16 KiB are
  refused. **Set `WEBHOOK_SECRET` in your `.env` before upgrading**, to the
  value on top.gg's webhook page.

### Added

- **`/status`**, for anyone in a server, shows privately: the server's plan
  (premium, or free with its 24 hours of queue history); the bot's version and uptime;
  what's playing here (channel, title, length and how many more are queued, or idle,
  or just that a `/gp` game is on, since its titles are the answers); and the settings
  in effect (idle timeout, which premium servers never hit, volume, autopause, and this
  session's autoplay).
- **Bot owners can see any server's queue history on the dashboard**, for debugging
  and support, whether or not they're a member or have Manage Server there. The owner
  set is the one `owners_only` commands use, built by one function for both.
- **`/premium status`** shows members with Manage Server whether their server has
  premium, privately: a thank-you for premium servers, and for free ones what free means
  (24 hours of queue history) with the Patreon plug. `/premium` is now listed to members
  with Manage Server instead of administrators; `grant` and `revoke` stay owner-only.
- **`/premium grant` and `/premium revoke`** (bot owners only) turn premium on or off
  for any server the bot is in, by server id, from wherever the owner runs it. Before
  this there was no working way to grant premium: `/set premium` is not registered,
  because the whole `/set` command is switched off, and a database edit made while the
  bot runs is overwritten at shutdown. The v0.19.0 docs wrongly pointed at
  `/set premium true`; they now describe `/premium`.
- **A thank-you message for premium servers** (`CrackedMessage::PremiumThanks`), which
  `Plan::plug` picks in place of the Patreon plug. Nothing sends either yet.
- **The dashboard has a queue history page.** Members with Manage Server see a
  "History" link on a server's queue page. It lists who changed the queue, how
  and when, filters by member, action, source and time, loads older entries, and
  shows new ones within 10 seconds. A running `/gp` game's entries stay hidden
  until it ends.

- **Every `/gp` round ends with its results.** Once a round's last song has been
  revealed, and before the next prompt goes up, a round-results embed sums the
  round up at the bottom of the channel: each song with who submitted it, who
  guessed it and its 👍, with "fooled everyone" and "played in full" marked;
  what the round paid each player; and the scoreboard. Each song's own reveal is
  still an edit of that song's message, which after five songs left the results
  scattered up the channel behind the next prompt. The five-second breather
  that already sat between songs now sits between the results and the next
  prompt too. A round nobody submitted to has nothing to sum up and posts none.
  `/gp start ... results:false` turns the embed off and has the game as it was,
  except with `reveal:round`, where the results are the reveal and stay on.
  The results survive a restart: the game is written down again once they are
  posted, and a bot that comes back to a round that never got them -- or to a
  game that finished without its last results and scoreboard going up -- posts
  them first. (#433)
- **Submitters are now revealed at the end of the round, not after each song.**
  Every name is held until the round's last song has played, and the
  round-results embed is the reveal. With the reveal after each song, the last
  song of a round is never a guess
  -- everyone has one song in, so by the final one the room knows by
  elimination, and with three players the second is a coin flip. While a round
  is held, a song's end shows only that it is over and its 👍; the scoreboard on
  it, and on `/gp status`, stays as it stood when the round began, since a
  player up a hundred after song one either guessed it or was the one nobody
  guessed. `/gp start ... reveal:song` is the old reveal after each song. (#450)

- **`/gp` games survive a restart.** A game is written to Postgres each time a
  song is submitted and each time a song ends, and a bot that comes back within
  five minutes picks it up where it was: a submission window with whatever time
  was left on it, or the song that was playing from the top, with the scoreboard
  intact. Nothing on the interaction or playback path waits on the database; a
  single writer task drains the writes in order, and shutdown drains it before
  the pool closes. A game down longer than five minutes, or whose voice channel
  has emptied, is not brought back -- its scoreboard as it stood is posted with a
  line saying it was lost to an outage. Guesses and 👍 on the song that was
  playing are not saved: that song plays again and the room casts them again.
  Every game ever played stays in the `gp_*` tables as history. Without
  `DATABASE_URL` the game runs in memory exactly as before. (#431)

- **Spotify links play.** Pasting a Spotify track, album or playlist link into
  `/play` or `/gp submit` now resolves it instead of failing. Resolution goes
  through [sleevenote](https://github.com/cycle-five/sleevenote), which needs no
  Spotify credentials; the songs themselves are still found on YouTube, exactly
  as the old path did. `/spotify` and `/playlist loadspotify` were moved onto the
  same resolver, so all four commands now agree about what a link means and
  report a failure the same way.
- A Spotify failure now says which failure it was: not configured, unreachable,
  no such link, lookup broken, or timed out. Previously a Spotify link in
  `/gp submit` produced a **yt-dlp** error about a URL yt-dlp was never going to
  be able to fetch, and one in `/play` produced a Spotify *auth* error.
  Podcast episodes inside a playlist are skipped rather than guessed at, and a
  link that resolves to nothing playable says so.
- `SLEEVENOTE_BASE_URL` is documented, including what its localhost default
  means in Docker Compose, where the bot's own localhost is not the sleevenote
  container.
- **Buttons on the now-playing message.** Pause or Resume, Skip, Repeat and
  Shuffle, as symbols only (⏸️ ▶️ ⏭️ 🔁 🔀) so the row stays on one line on a
  phone, for anyone who may use the music commands there (the music channel
  and `/gp` rules apply). A press posts an echo line naming who pressed it, as
  a dashboard control does, and the status message updates below it. Skip names
  the song it was drawn for, so an old message or two people pressing at once
  cannot skip the next song. Buttons on an old message still work after a
  restart. Restrictions set on individual commands under Server Settings →
  Integrations do not cover the buttons: the bot's own music-channel and `/gp`
  rules do.
- **`/echoes`** (admins) turns those echo lines off or on for the server, for
  buttons and the dashboard alike. On by default.
- The queue history's source filter (`/auditlog`, the dashboard) has `button`.

### Changed

- **The now-playing message shows when the track ends**, counted down live by
  Discord ("4:33 · ends in 3 minutes"); "Paused at 1:12" while paused; "4:33 ·
  on repeat" while the track repeats, since it has no end; and "Started … ago"
  for a live stream. `/pause`, `/resume`, `/seek` and `/repeat`, and the
  dashboard's pause, resume and repeat, bring it up to date and move it below
  their reply, as a skip does. Ticking in real time between events is a planned
  follow-up.
- **Durations in music messages read `m:ss`, or `h:mm:ss` from an hour up**
  (`/playytplaylist`'s lines, search results and `/spotify`'s track length
  included), and an unknown one is left out. `/gp` and `/uptime` keep the old
  `00:00:00` style until they move to the new layer.
- **Queue pages show titles in bold** (`[**title**](url)`, was `[title](url)`),
  like the other music messages.
- **Replies and notices carry a colour stripe by kind:** errors red, general
  notices gold (the idle-leave alert, the autoplay notices, the playlist
  progress line), the rest blue. Cards (now playing, queued, the "Finished"
  status), dashboard echo lines, the failed-track notice and embeds a command
  builds itself carry none. Before, most replies had no colour.
- **The autoplay notices and the idle-leave alert are embeds** like every other
  notice.
- **Music messages can't ping @everyone, a role or a user by accident.** Every
  message sent through the new layer states who it may ping, and the default is
  nobody, so an `@everyone` in a track title stays text. Sends not migrated yet
  (the welcome message, camera enforcement, `/gp`, admin) are unchanged.
- **Every music message now goes through one renderer and one delivery path**,
  and clippy refuses raw Discord sends outside `crack-core::messaging`. The
  modules not yet migrated (`/gp`, admin, settings, utility, ...) are marked and
  move in follow-ups.
- **Free servers now see 24 hours of queue history.** The dashboard's history page and
  `/auditlog` show free servers the last 24 hours, and premium servers everything. When
  older entries exist, both say older history is a premium feature and link the
  Patreon. A server whose settings haven't loaded counts as free.
- **The Patreon plugs are reworded.** They no longer say premium gates nothing. The idle
  alert, which premium servers never see, now says premium keeps the bot in the voice
  channel as long as you like.
- **`/gp voteskip` and `/gp votefull` no longer say who voted.** Both answered
  in public as a slash-command reply, which Discord renders under "*name* used
  `/gp voteskip`" -- so the room saw exactly who wanted the song gone, in a game
  whose whole premise is that people submitted something embarrassing. The
  confirmation (and any error, such as "you already voted") now goes to the
  voter alone, ephemerally, the way `/gp submit` answers; the room gets a plain
  message in the game's channel that names nobody: "someone voted to skip this
  song -- *n* more and it's gone". A vote that carries, or a submitter pulling
  their own song, is still announced to the room, but as a channel message
  rather than a reply, so the last voter is not named on that either. The
  prefix form of either command answers the voter by DM, since a prefix
  invocation has no ephemeral reply -- though the `!gp voteskip` message itself
  is in the channel, so the slash form is the one that keeps a vote to
  yourself. (#433)

- `/gp submit` refuses an album or playlist link rather than silently submitting
  its first track. Which song a player submits is the whole game, so choosing one
  for them would replace their move with ours and they would never know.
- The rspotify Spotify path is retired. It could not authenticate -- Spotify
  stopped issuing Web API credentials around December 2025 -- and every
  resolution path now goes through sleevenote, so ~240 lines of unreachable
  extraction code are gone. What remains of rspotify is autoplay's
  recommendations, which sleevenote has no endpoint for and which already
  degrades with a message saying so.

### Fixed

- **A track with no title read "⏭️ Skipped to **!"**; it now reads
  "(untitled)". Blank queue lines, playlist lines, the dashboard's echo lines
  ("⏭ Skipped **** from the dashboard") and search-menu labels do too; an empty
  search-menu label would have made Discord reject the whole menu.
- **Titles with `*`, `_`, backticks, brackets, `<` or `@` broke the message's
  formatting or could ping** (`@everyone` in a title). Titles are now escaped
  everywhere they appear: skip, queued, now playing, queue pages, `/nowplaying`'s
  pointer, `/playlog`, `/myplaylog`, `/spotify`, `/playytplaylist`, failure
  notices and the dashboard's echo lines.
- **"Track duration: 00:00" and the "Estimated time until play" built on it no
  longer appear when a length is unknown.** The estimate itself was wrong: it
  counted the playing track's whole length once per track ahead, could show "∞"
  for a live stream, and could hang reading track info. It now sums each track's
  own remaining length, and is left out when any length is unknown.
- **A title is a link only when its URL is a real http(s) URL.** No more
  `[**x**]()` links to nowhere, and parentheses in a URL no longer cut the link
  short.
- **No more `RelativeUrlWithoutBase` errors in the log, or "Streaming via
  unknown" footer,** for tracks without a link or thumbnail. A thumbnail is set
  only from an http(s) URL, on `/queue`, `/remove` and `/spotify` too.
- **`/queue` no longer fails on a page of long links.** Six tracks with
  SoundCloud-length links could pass Discord's 1024-character limit for a
  field, and Discord rejected the whole reply. Every track on the page is
  still listed: when the links don't fit, the last lines show their title
  without the link.
- **A `/search` pick no longer fails when its results menu can't be deleted.**
  The pick plays, and the leftover menu is logged.
- **`/search` no longer panics on long titles in Japanese, emoji and the like.**
  Labels were cut by bytes, not characters.
- **`/play` in play-all, reverse or shuffle mode with plain keywords answered as
  if it had worked** ("Now playing ...", "No tracks in queue!"). It now says it
  can't do that, with the existing play-all failure text, and removes the
  "🔎 Searching..." placeholder.
- **`/grab` said "grabbed" even when your DMs were closed.** It now says "Could
  not send you a DM. Check that your DMs are open." With nothing playing it says
  so as an error instead of sending a "Nothing playing" DM.
- **Plain-text replies no longer carry stray ANSI colour codes.** They were
  invisible only because the container's stdout is not a terminal.
- **A track that could not play now says so.** songbird ends a track it cannot
  open or decode, and the queue moved on in silence: SoundCloud's undecodable
  streams before v0.20.1 just emptied the queue. The music channel (or the voice
  channel's chat) now gets "⚠️ Couldn't play **Title**: that format isn't
  supported". Failures within 30 seconds of each other edit one message, so a
  playlist of dead links is one notice listing ten tracks and counting the rest.
  The raw error goes to the log, never to Discord. A `/gp` game reports its own
  dead songs and is left alone.
- **A track whose stream YouTube refuses gets a second try.** YouTube now and then
  answers the URL yt-dlp just resolved with `403 Forbidden` (3 of ~45 track starts on
  production over a day), and the track was skipped before a note played. Every queued
  track now re-resolves once when its stream URL is refused, which gets yt-dlp a fresh
  URL; only a second refusal skips it, with the notice above.
- **Joining voice reset a server's settings in memory to defaults, until the next
  restart.** `get_or_create_guild_settings` built its defaults in the argument to an
  eager `unwrap_or`, so it ran on every call and stored the defaults over the settings
  loaded from the database, while returning the old copy. Every voice join (and
  `/volume`) did this: premium switched off, and volume, idle timeout, autopause and
  prefix fell back to defaults. The stored settings were never touched, because the
  defaults are marked as not loaded from the database and the shutdown save skips
  them. `/premium grant` on such a server then reloaded the stored row with the
  defaults' empty name, which blanked the server's stored name; the next restart
  writes it back from Discord. Present since v0.3.16.
- **A keyword `/play` now asks yt-dlp whenever rusty_ytdl can't answer.** It
  fell back only when rusty_ytdl found nothing; an error from it returned
  straight away, and the next fallback (`ready_query`) searched with rusty_ytdl
  again, so a rusty_ytdl outage failed every keyword play. Playlists, Spotify
  lists, `/gp` and autoplay resolve through the same path and get the same
  fallback. yt-dlp is slower, but it is kept up to date with YouTube.

- **A `/gp` round could play its songs in the same order as the round before.**
  Each round's order was an independent uniform shuffle, which with three
  players repeats the previous round's order one time in six and keeps someone
  in the same slot two times in three -- and once a room has noticed, the
  position in the round says whose song it is before a note has played. The
  order is now drawn against the previous round: nobody keeps the slot they had
  last time. Two-song rounds are left alone, since forbidding the repeat there
  would make the rounds alternate, which is a tell of its own. (#450)
- Scoring is now one function of a song as it stands -- guesses, likes, the
  full-song vote, and whether it played at all -- used by the reveal, the round's
  results and the held-back scoreboard alike, so they cannot disagree. Whether a
  song failed to play is saved with the game, so a resumed game still pays
  nothing for it.

- Locale-prefixed Spotify links -- `/intl-de/track/<id>`, which is what
  Spotify's own web player hands out to much of the world -- were rejected as
  invalid. The old regex read `intl-de/track` as the entity kind.
- `spotify:album:<id>` and `spotify:playlist:<id>` URIs were rewritten into
  `/track/<id>` URLs and looked up as tracks, which found nothing. The kind is
  now read from the URI.
- A dashboard control that changed nothing (pausing a paused song from a stale
  tab) no longer posts an echo line.
- Outside the music channel, the refusal now names the music channel to use,
  not the channel you are already in.
- Pasting the dashboard's address in Discord previewed it as Discord's own
  "CrackTunes • Discord App" sign-in card: a signed-out visit to `/` redirected
  to the login, and Discord followed the redirect to its OAuth page. `/` now
  answers signed-out visitors with a landing page that carries its own preview
  (title, description and image) and a "Sign in with Discord" link. (#589)

## TODO:

- [ ] /changenicks command. Renames all users in the guild
      to a random nick name from a themed list of names. Use your
      own custom list, or choose from one of the many I've
      pre-curated and use in my own server.
- [ ] Codebase architecture documentation.
- [ ] Support discordbotlist.com (voting service).
- [ ] Decide on whether to use ephemeral for admin messages.

## v0.6.3 (2026/09/07)

### Added

- **`/gp` plays a clip of each song** rather than the whole thing: 45 seconds starting 30
  seconds in, by default. The first half-minute of a song is usually an intro that gives a
  guesser nothing, so skipping it makes the clip *more* guessable, not less, and a round
  moves at the pace of a party game. `/gp start ... clips:false` plays songs whole;
  `clip_start` and `clip_length` tune the clip and are ignored when `clips` is off. A song
  shorter than the offset is played from as late as it can be rather than seeked past its
  own end, which would come back as an immediate `End` and read as a dead link.
- **`/gp votefull`** — a majority of the voice channel votes to hear the current song in
  full instead of just its clip, and the submitter takes +50 for it. Same pool as
  `/gp voteskip`; the submitter cannot vote for their own song, since that is voting
  themselves the bonus.

### Changed

- The "did the room actually hear it" bar is now relative to how much of the song was meant
  to play -- half of it, capped at the previous flat 30 seconds. A 45-second clip needs 22,
  where the absolute rule would have wanted two thirds of the clip and a 20-second clip
  could never have cleared it at all. The dead-link versus fooled-everyone split in #423 is
  a separate question and is untouched.

### Fixed

- **Guild settings never loaded on a bot in more than a handful of guilds.**
  Settings were loaded from `CacheReady`. serenity emits that event from inside
  `GuildCreate` handling, and only when `cache.unavailable_guilds` has drained to
  exactly zero -- every guild in the `Ready` payload having checked in. A single
  guild that is down at startup, or one the bot was removed from while offline,
  and it never fires at all for the life of the process. `GuildDelete` puts a
  guild back into that set, so even a cache that completes once can lose the
  condition permanently.

  At thirteen guilds that barrier clears every time. At a hundred and fifty it
  effectively never does -- measured on the deployed bot, where 155 `GuildCreate`
  events arrived and `CacheReady` never fired once.

  Settings now load per guild from `GuildCreate`, which is also correct for guilds
  that recover from an outage or are joined while the bot is running. The camera
  status loop moved to `Ready`, which always fires and already carries the guild
  ids it needs.

  Latent rather than active: with no `DATABASE_URL` there are no stored settings
  to miss, and defaults are materialised lazily on first use. With a database
  configured it would have been destructive -- settings never loaded, defaults
  created lazily, and the shutdown handler writing those defaults back over the
  stored rows on every restart.

## v0.6.2 (2026/09/06)

### Fixed

- **Guild settings never loaded when running without a database.** The ready
  handler unwrapped `Data::database_pool`, which is `None` whenever
  `DATABASE_URL` is unset -- a configuration the bot otherwise supports and the
  one production runs in. The unwrap panicked once per guild, so no guild ever
  got settings, and every one silently fell back to library defaults for its
  prefix and everything else.

  It panicked on a Tokio worker rather than the main thread, so the process
  survived and the bot looked healthy. That is why this went unnoticed: the
  symptom was indistinguishable from the graceful degradation that was intended.

  Settings are now built from defaults and overlaid from the database when one
  is configured, which is the shape the older `_load_guilds_settings` already
  used. A database error falls back to defaults rather than taking the task
  down, and a missing pool is reported once at startup instead of being silent.

## v0.6.1 (2026/09/05)

### Fixed

- **Nothing played.** Every track failed with songbird reporting `Preparing -> Errored`
  and `play_time: 0ns`, which `/gp` faithfully reported as "never played" and which the
  music commands reported as nothing at all. Three separate faults, all in the path
  between a resolved track and audio:
  - `rusty_ytdl` was pinned to a fork last pushed 2025-02-01, which asks YouTube's
    `youtubei/v1/player` with the WEB client context. That now returns HTTP 400. Repinned
    to upstream `bfd7fed` ("use android_sdkless player by default"), which gets past it.
  - Past the 400, the googlevideo URL `rusty_ytdl` hands back is fetched as `c=ANDROID`
    and returns HTTP 403. songbird then has an empty stream and symphonia reports
    `probe reach EOF at 0 bytes` followed by `no suitable format reader found` -- which
    reads like a codec fault and is a fetch fault. Playback now goes through songbird's
    `YoutubeDl` (yt-dlp), whose URL serves 206.
  - yt-dlp could never have run anyway: the image installed `yt-dlp_linux`, the glibc
    build, onto an Alpine (musl) base, so the binary was present and unexecutable. It is
    `yt-dlp_musllinux` now, and the build asserts `yt-dlp --version` so choosing the wrong
    build fails the build rather than the bot.
- `deno` is in the image. YouTube extraction without a JavaScript runtime is deprecated
  upstream and silently drops formats. Alpine's build is musl-native, unlike deno's own
  releases.

### Changed

- Every playback path now builds its songbird `Track` through `build_track`.
  `queue_resolved_track_back` and `resolve_query_to_tracks` each constructed their own
  source inline, so changing the source in `build_track` fixed `/gp` and left `/play`
  silent. One construction site now, and the duplicated `TrackData` assembly is gone.

## v0.6.0 (2026/09/05)

### Added

- **`/gp` — "What's your song?" party game** (alias `/guiltypleasure`, category Games).
  The host runs `/gp start <category> [rounds] [timer]` in a voice channel, picking one of
  seventeen prompt categories (🥹 Nostalgia, 🔥 Slightly More Dangerous, 🎶 The Really Good
  Game Prompts, 🚗 Car / Driving, 🌿 Altered-State / Chill, 😭 Emotional, 🤢 Bad Music,
  🎧 Hyper-Specific, 🖤 Weirdly Revealing, 😂 Game Chaos, ⚡ One-Worders, 🎤 Social / Go-To,
  😈 Guilty Pleasures / Secret Taste, 💋 Sex / Romance / Attraction, 🥀 Emotional Damage,
  🕺 Chaotic / Funny, 🧠 Personality Reveals) or 🎲 Mixed.
  Each round the bot posts a prompt ("What song do you cry to?") with a live countdown;
  everyone in the voice channel secretly submits one song with the ephemeral, slash-only
  `/gp submit <song>` (resubmitting replaces it). The window closes on the timer (default
  3 minutes, 30-second warning), as soon as every non-bot member of the voice channel has
  submitted, or when the host runs `/gp close`. The round's songs then play back-to-back
  with the requester hidden ("(auto)" in the now-playing/queue embeds); under each song a
  dropdown asks who submitted it and a 👍 button lets people like it. When the song ends
  the message is edited with the reveal, likes and scores. `/gp skip` (host) ends a song
  early, `/gp status` shows the prompt, who has submitted/guessed, likes and scores,
  `/gp end` (host or Manage Server) aborts. Scoring: +100 per correct guess, +100 to the
  submitter if nobody guessed them, +10 per 👍. A one-song round is likes-only; an empty
  round is skipped. Scores are in memory for the game only. While a game runs,
  music commands that would take playback out of the game's hands (`play`, `skip`,
  `voteskip`, `stop`, `pause`, `seek`, `repeat`, `leave`, `summon`, ...) are refused with
  a pointer to `/gp skip` / `/gp close` / `/gp end`, autoplay and autopause are
  suspended, and the game is discarded if the bot is disconnected from voice. `/resume`
  is deliberately left open as an escape hatch. Prompts are compiled in from
  `crack-core/src/commands/music/gp_prompts.json`.
- `/gp voteskip` ends the current song once a strict majority has voted, so a song can be
  moved past without the host. The song's own submitter does not vote on it: running the
  command on your own song pulls it outright, and the submitter is left out of the pool
  the majority is measured against, so a two-person channel still only needs the one
  eligible voter. Votes belong to the song and are cleared with it.
- The game is closed to spectators: acting on a running game requires being in its voice
  channel *and* having submitted a song. Submitting is how you join, and it sticks for the
  rest of the game, so sitting a round out does not put you back outside it. This applies
  to the guess dropdown and 👍 as well as to the subcommands, so someone who never submits
  can no longer guess their way to winning a game. `/gp end` is exempt from both checks so
  an admin can always stop a game from outside it, and the host can read `/gp status`
  before submitting.

### Changed

- `TrackEndHandler` now forgets skip votes before applying autopause, rather than after.
  The two are independent, and the order changed only so the `/gp` early-return could sit
  between them; guilds that never use `/gp` see no behaviour change.
- `IdleHandler` no longer counts a guild as idle while a `/gp` game is running. A
  submission window is silent by design, and at the default ten-minute idle timeout a long
  window would disconnect the bot mid-round and take the game with it.

### Fixed

- Ending a game no longer starts autoplay. `/gp end` removed the game before stopping
  playback, so the resulting `TrackEvent::End` reached the global handler with no game to
  find and was treated as an ordinary track ending -- in an autoplay guild, dropping an
  unrelated recommended track in on top of the game ending. Because `TrackHandle::stop()`
  only queues the `End` for the driver, the game now stays in the map until that event
  actually lands and the global handler collects it; `/gp end` no longer removes it itself,
  with a short backstop for the case where the event never arrives.
- A song whose stream dies part-way through is scored normally again. Any `Errored` state
  counted as "never played", so a stream that failed two minutes in discarded everyone's
  guesses and 👍 and told the room it never played. A failure now only skips scoring if
  less than 30 seconds played -- long enough that "the room heard it" is true, rather than
  paying the submitter the fooled-everyone bonus for a song that died in the first
  fraction of a second and nobody could have guessed.
- `/gp voteskip` could not reach a majority in a channel where anyone had not submitted.
  Only players may vote, but the majority was measured against the whole voice channel, so
  the bar counted people who could not help clear it: one non-submitter in a channel of
  three made the song unskippable by vote. The pool is now the players in the channel other
  than the song's submitter.

## v0.4.1 (2026/08/23)

### Security

- Cleared the four Dependabot alerts v0.4.0 left behind. protobuf 2.28 -> 3.7.2
  (via prometheus 0.13 -> 0.14), idna down to 1.1 only (via whois-rust 1.6 -> 3.1),
  and lru 0.12.5 -> 0.16.4 by dropping the `cycle-five/ipinfo-rs` fork for upstream
  ipinfo 3.5 -- upstream had since removed the openssl dependency the fork existed
  to avoid. The fourth, a libcrux panic, is unreachable: davey supports only the
  AES-128-GCM ciphersuite, so ChaCha20Poly1305 is never selected.
- Every workflow now declares an explicit `permissions:` block.

### Dead features repaired

Three optional features had rotted into a state where turning them on would not
compile. `-A warnings` and a lint workflow that had been auto-disabled for
inactivity meant nothing complained.

- **`crack-metrics`** was `[]`, so it never enabled `dep:prometheus`;
  `pub mod metrics;` was commented out of lib.rs, so `crate::metrics` did not exist
  for `utils.rs` to import; and the module held a private, unreferenced
  `metrics_handler` returning a `warp::Reply`, a crate crack-core does not depend on.
- **`crack-telemetry`** was `[]` as well, while `init_telemetry` referenced
  `JsonStorageLayer`, an undefined `formatting_layer`, and an OTLP
  `set_text_map_propagator` -- all left dangling when the opentelemetry imports were
  commented out. It now means structured JSON logs: `tracing-bunyan-formatter`
  behind the feature, with `SERVICE_NAME` finally used as the bunyan service name.
  The propagator call is gone rather than pulling in opentelemetry to feed a tracer
  nothing reads.
- **`crack-metrics` in crack-cli** was an empty feature that gated one dead const;
  it now forwards to `crack-core/crack-metrics`.

`cargo clippy --all-features` is clean for the first time in a long while, and
`rust-clippy.yml` -- disabled by GitHub for inactivity since 2025-11-30 -- is
enabled again now that it has something green to report.

## v0.4.0 (2026/08/22)

### Toolchain

- Builds on **stable Rust 1.98** (`rust-toolchain.toml` was pinned to `nightly`).
  The only nightly feature in use was `fmt_internals` / `formatting_options`,
  and only inside one test that Debug-formatted an embed.
- `.cargo/config.toml` no longer sets `-A warnings` (it was hiding every lint in
  the workspace) or `--cfg proc_macro_c_str_literals` (nothing reads it now).
- Dockerfile builder moved to `rust:1.98.0-alpine3.22`.
- **The workspace is clippy-clean again**, which it had not been in a long time.
  With `-A warnings` gone, `cargo clippy --all -- -D warnings` -- what the lint
  workflow actually runs -- surfaced roughly 120 warnings, and `cargo fmt --check`
  another 33 hunks. Notable substance behind the noise: six `unnecessary_unwrap`
  sites that unwrapped a value right after testing it (the voice-state handler's
  channel-change branch is now a `match` on the pair, and no longer panics when a
  member is missing from both the old and new state), stale `#[allow]`s for a lint
  clippy has since removed, and dead imports. The three `large_enum_variant`
  offenders (`QueryType`, `MessageOrReplyHandle`, `CommandOrMessageInteraction`)
  carry a documented `#[allow]`: boxing them is worth doing, but it touches every
  construction and match site and belongs in its own change.

### Dependencies

- **Dropped the `CycleFive/*` forks of serenity, songbird and poise** in favour
  of upstream. The serenity mirror had no custom commits and was ~30 behind;
  the songbird mirror pointed at the identical upstream commit; the poise fork
  differed only by a stale `rev` pin. Since songbird 0.6 and poise both resolve
  serenity from `serenity-rs/serenity`, keeping the mirror forced two
  incompatible copies of serenity into the dependency graph.
- Removed the `[patch.crates-io.serenity-voice-model]` entry: that subcrate no
  longer lives in the serenity repo, and the patch broke resolution outright.
- serenity 0.12.5-next, songbird 0.4.5 -> 0.6, serenity-voice-model 0.2 -> 0.3,
  tokio 1.42 -> 1.53, sqlx 0.8.2 -> 0.8.6, async-openai 0.26 -> 0.41 (dropping
  the fork and the `backoff` patch), vergen-gitcl 1.0 -> 10.0, extract_map
  0.1 -> 0.3.

### API migration (serenity `next`, songbird 0.6, poise)

- `ChannelId` split into `ChannelId` (guild channels) and `GenericChannelId`
  (anything you can send a message to).
- `EventHandler`'s per-event methods collapsed into a single `dispatch`;
  poise dropped `FrameworkOptions::event_handler`, so the event log/router is
  driven from the serenity handler now.
- Components v2: top-level components are `CreateComponent`, with action rows
  as one variant.
- `MessageUpdateEvent` is now `{ message }`; `GuildChannel` gained a flattened
  `base`; `FullEvent::snake_case_name()` gave way to `strum::IntoStaticStr`.
- poise no longer accepts `usize` command arguments; affected commands take
  `u32` and convert at the boundary.

### Fixed: playlists

- **Playlists had stopped resolving entirely.** `rusty_ytdl` 0.7.4 looks for
  `playlistVideoRenderer` entries in `ytInitialData`; YouTube has since moved
  playlist listings to `lockupViewModel`, so every playlist failed with
  `PlaylistBodyCannotParsed`. Added `crack-testing`'s `yt_playlist` module,
  which reads the playlist page directly, understands both shapes, and follows
  continuations (also moved, to `continuationItemViewModel`) for playlists
  longer than one page. rusty_ytdl remains the fallback.
- **Playlist loading is no longer serial.** The play path used to discard the
  metadata the playlist fetch had already returned and re-resolve every entry
  one at a time, spawning a `yt-dlp` subprocess per track. Entries now carry
  their metadata straight from the listing, and anything that does need
  resolving (keyword lists, Spotify tracks) goes through `resolve_track_many`,
  which runs `RESOLVE_CONCURRENCY` lookups at a time and preserves order.
- Keyword resolution no longer follows its search hit with a redundant
  `get_info` round trip -- that doubled the cost of every Spotify playlist
  track for metadata the search already returned.
- The first track of a playlist is queued on its own so playback starts
  immediately, and progress edits are throttled instead of one-per-batch
  (Discord rate-limits edits per channel).
- A single unresolvable entry (deleted, private, region-locked) is skipped and
  logged instead of aborting the whole playlist load.
- Batch enqueues take the call lock once rather than once per track.

## v0.3.16 (2024/12/12)
- Commands each show up and work only where they are supposed to (guilds, dms, etc).

## v0.3.16-alpha.3 (2024/12/09)
- re-enable the commands that were disabled in the last release
  for the serenity-next branch.
- Got the rusty_ytdl library with the compose to an Input working.
  The result is the bot starts up and responds and queues songs much faster.
- Youtube suggestions are now working again.

## v0.3.16-alpha.2 (2024/12/01)
- [x] update to serenity-next branch

## v0.3.15-alpha.1 (2024/11/23)
- [x] bug fix patch 

## v0.3.14 (2024/11/05)
- [x] Big refactor, moving a lot of the code into modules.
- [x] crack-testing module for testing and developing new features without
  affecting the main bot.
- [x] crack-types module for shared types. New modules can depend on this
  to avoid circular dependencies.
- [x] Auto complete for `/play` brings up actual youtube search results.

## v0.3.13 (2024/09/19)
- Dependency updates

## v0.3.12 (2024/09/12)

- [x] `/movesong` command
- [x] `muteall` command to server mute all other people in a call (Admin only)
- [x] `@bot` mention works like a prefix.
- [x] default to playing the album version of songs where possible.
- ~~[ ] Add setting for whether or not to look for album version of song.~~ (reverted moved to next release)
- [x] Large refactoring of code into more modules
- [x] Test Coverage > 24%.

## v0.3.11 (???)

- ???

## v0.3.10 (2024/07/28)

- [x] performance improvements.
- [x] All milestones recorded as GitHub issues.
- [x] Add help option to all commands.
- [x] Added back in internal playlist support. 
- [x] `/playlist create <playlistname>` Creates a playlist with the given name
- [x] `/playlist delete <playlistname>` Deletes a playlist with the given name
- [x] `/playlist addto <playlistname>` Adds the currently playing song to <playlistname>
- [x] `/playlist list` List your playlists
- [x] `/playlist get <playlistname>` displays the contents of <playlistname>
- [x] `/playlist pplay <playlistname>` queues the given playlist on the bot
- [x] `/playlist loadspotify <spotifyurl> <playlistname>` loads a spotify playlist into a Crack Tunes playlist.

## ~~v0.3.9~~

- internal testing version, publicly skipped
- i.e. git branches got fucked and this was easier

## v0.3.8 (2024/07/17)

- [x] Looked at rolling back to reqwest 2.11 because it was causing problems.
      Decided to stick with 2.12 and keep using the forked and patched version
      of serenity, poise, songbird, etc.
- [x] Pulled in songbird update to support soundcloud and streaming m8u3 files.
- [x] More refactoring.
- [x] Brainf\*\*k interpreter.
- [x] Switched all locks from blocking to non-blocking async.
- [x] Unify messaging module.
- [x] Fixed repeat bug when nothing is playing.
- [-] Change `let _ = send_reply(&ctx, msg, true).await?;`
  to `ctx.send_reply(msg, true).await?;` (half done)
  ...
  For next version...

## v0.3.7 (2024/05/29)

- crackgpt 0.2.0!
  Added back chatgpt support, which I am now self hosting for CrackTunes
  and is backed by GPT 4o.
- Use the rusty_ytdl library as a first try, fallback to yt-dlp if it fails.
- Remove the grafana dashboard.
- Switch to async logging.
- Add an async service to handle the database (accept writes on a channel,
  and write to the database in a separate thread).
  Eventually this could be a seperate service (REST / GRPC).

## v0.3.6 (2024/05/03)

- Music channel setting (can lock music playing command and responses to a specific channel)
- Fixes in logging
- Fixes in admin commands
- Lots of refactoring code cleanup.

## v0.3.5 (2024/04/23)

- Significantly improved loading speed of songs into the queue.
- Fix Youtube Playlists.
- Lots of refactoring.
- Can load spotify playlists very quickly
- Option to vote for Crack Tunes on top.gg for 12 hours of premium access.

## v0.3.4

- playlist loadspotify and playlist play commands
- Invite and voting links
- Updated serenity / poise / songbird to latest versions
- Refactored functions for creating embeds and sending messages to it's own module

## v0.3.3 (2024/04/??)

- `/loadspotify <spotifyurl> <playlistname>` loads a spotify playlist into a Crack Tunes playlist.
- voting tracking

## v0.3.2 (2024/03/27)

- Playlists!
- Here are the available playlist commands
  - `/playlist create <playlistname>` Creates a playlist with the given name
  - `/playlist delete <playlistname>` Deletes a playlist with the given name
  - `/playlist addto <playlistname>` Adds the currently playing song to <playlistname>
  - `/playlist list` List your playlists
  - `/playlist get <playlistname>` displays the contents of <playlistname>
  - `/playlist play <playlistname>` queues the given playlist on the bot
- Added pl alias for playlist
- Added /playlist list
- Fixed Requested by Field
- JSON for grafana dashboards

## v0.3.1 (2024/03/21)

- Fix the requesting user not always displaying
- Reversed order of this Change Log so newest stuff is on top

## ~~0.3.0-rc.6~~

## 0.3.0

- Added more breakdown of features which can be optionally turned on/off
- Telemitry
- Metrics / logging
- Removed a lot of unescesarry dependencies

## 0.1.4 (crack-osint) (2024/03/12)

- osint scan command to check urls for malicious content

## 0.3.0-rc.5 (2024/03/09)

- cargo update
- GuildId checks
- user authorized message
- adding scan command
- add feature for osint
- make admin commands usable by guild members with admin
- add dry run to rename_all

## 0.3.0-rc.4

- fix storing auto role and timeout I think
- download and skip together
- ~~try to finally fix this fucking volume bug~~
- fix loading guild settings
- add pgadmin to docker compose
- ~~fix volume~~ (volume is still broken)

## 0.3.0-rc.2

- [x] Clean command
- [x] Bug fixes
- ~~[ ] Down vote~~ (not working)

## 0.3.0-rc.1

- [x] Dockerized!
- [x] Refactored settings commands.
- [x] Storing and retrieving settings from Postgres.
- [x] Updated dependencies to be in line with current.

## ~~0.2.13~~

- ~~[] Port to next branch of serenity~~
- ~~[] Flesh out admin commands~~

## ~~0.2.12~~

## ~~0.2.6~~

Didn't really track stuff here...

## 0.2.5

- ~~[] Shuttle~~
- ~~[] Reminders~~
- ~~[] Notes~~

## 0.2.4 (2023/07/17)

- [x] Bug fixes.
- [x] Remove reliance on slash commands everywhere.
- [x] Remove shuttle for now

## 0.2.3

- [x] Bug fixes (volume)
- [x] Shuttle support (still broken)

## 0.2.2 (2023/07/09 ish)

- [x] Welcome Actions
- [x] Play on multiple servers at once

## 0.2.1 (2023/07/02)

- [x] Play music from local files

## 0.2.0

- [x] Play music from YouTube
- [x] Play music from Spotify (kind of...)
