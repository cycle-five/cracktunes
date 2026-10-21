pub const ADDED_QUEUE: &str = "📃 Added to queue!";
pub const AUTOPAUSE_OFF: &str = "🤖 Autopause OFF!";
pub const AUTOPAUSE_ON: &str = "🤖 Autopause ON!";
pub const AUTOPLAY_OFF: &str = "🤖 Autoplay OFF!";
pub const AUTOPLAY_ON: &str = "🤖 Autoplay ON!";
pub const CLEARED: &str = "🗑️ Cleared!";
pub const CLEANED: &str = "🗑️ Messages Cleaned: ";
pub const CHANNEL_SIZE_SET: &str = "🗑️ Channel size set!";
pub const CHANNEL_DELETED: &str = "🗑️ Deleted channel!";
pub const COINFLIP: &str = "You flipped a coin and it landed on";
#[macro_export]
macro_rules! DICE_ROLL {
    ($number_of_dice:expr, $sides_per_die:expr, $res:expr) => {
        &format!(
            "You rolled {} dice with {} sides. Here are the results:\n{}",
            $number_of_dice,
            $sides_per_die,
            $res.iter()
                .map(|x| x.to_string())
                .collect::<Vec<String>>()
                .join(", ")
        )
    };
}

// pub const COMMAND_DISABLED: &str = "⚠️ Command is disabled!";
// pub const COMMAND_ENABLED: &str = "✅ Command is enabled!";

pub const AUTHORIZED: &str = "✅ User has been authorized.";
pub const AUTO_ROLE: &str = "Auto Role";
pub const BANNED: &str = "Banned";
pub const BUG: &str = "🐞 Bug!";
pub const BUG_END: &str = "was None!";
pub const BUG_REPORTED: &str = "🐞 Bug Reported!";
pub const BUG_REPORT: &str = "🐞 Bug Report";
pub const CONNECTED: &str = "Connected!";
pub const DEAUTHORIZED: &str = "❌ User has been deauthorized.";
pub const UNBANNED: &str = "Unbanned";
// Use the unicode emoji for the check mark
pub const EMOJI_HEADPHONES: &str = "🎧";
pub const DEAFENED: &str = "User deafened.";
pub const DEAFENED_FAIL: &str = "User failed to be deafened.";
pub const UNDEAFENED: &str = "Undeafened";
pub const UNDEAFENED_FAIL: &str = "User failed to be undeafened.";
pub const MUTED: &str = "Muted";
pub const UNMUTED: &str = "Unmuted";

pub const DOMAIN_FORM_ALLOWED_TITLE: &str = "Allowed domains";
pub const DOMAIN_FORM_BANNED_TITLE: &str = "Banned domains";
pub const DOMAIN_FORM_ALLOWED_PLACEHOLDER: &str =
    "Add domains separated by \';\'. If left blank, all (except for banned) are allowed by default.";
pub const DOMAIN_FORM_BANNED_PLACEHOLDER: &str =
    "Add domains separated by \';\'. If left blank, all (except for allowed) are blocked by default.";
pub const DOMAIN_FORM_TITLE: &str = "Manage sources";

pub const EMPTY_SEARCH_RESULT: &str = "⚠️ No search results found!";
pub const ERROR: &str = "Fatality! Something went wrong ☹️";
pub const EXTRA_TEXT_AT_BOTTOM: &str =
    "This is a friendly cracking, smoking parrot that plays music.";
pub const FAIL_ALREADY_HERE: &str = "⚠️ I'm already here!";
pub const FAIL_ANOTHER_CHANNEL: &str = "⚠️ I'm already connected to";
pub const FAIL_AUDIO_STREAM_RUSTY_YTDL_METADATA: &str =
    "⚠️ Failed to fetch metadata from rusty_ytdl!";
pub const FAIL_AUDIO_SOURCE: &str = "⚠️ Couldn't load audio from that link or search!";
pub const FAIL_AUTHOR_DISCONNECTED: &str = "⚠️ You are not connected to";
///?
pub const FAIL_AUTHOR_NOT_FOUND: &str = "⚠️ Could not find you in any voice channel!";
pub const FAIL_LOOP: &str = "⚠️ Failed to toggle loop!";
pub const FAIL_PAUSE: &str = "Failed to pause";
pub const FAIL_SEEK_OP: &str = "Failed to seek";
/// The driver did not confirm a seek within `ops::SEEK_TIMEOUT`.
pub const FAIL_SEEK_TIMED_OUT: &str = "The seek did not finish in time";
pub const ADD_UNPLAYABLE: &str = "That track could not be queued.";
pub const FAIL_SKIP: &str = "Failed to skip";

// The one-line echo the dashboard posts in Discord after a control.
pub const ECHO_SKIPPED: &str = "⏭ Skipped";
pub const ECHO_PAUSED: &str = "⏸ Paused";
pub const ECHO_RESUMED: &str = "▶ Resumed";
pub const ECHO_REPEAT_ON: &str = "🔁 Repeat on";
pub const ECHO_REPEAT_OFF: &str = "🔁 Repeat off";
pub const ECHO_REMOVED: &str = "🗑 Removed";
pub const ECHO_ADDED: &str = "➕ Added";
pub const ECHO_ADDED_NEXT: &str = "⏭ Up next:";
pub const ECHO_SHUFFLED: &str = "🔀 Shuffled the queue";
pub const ECHO_FROM_DASHBOARD: &str = "from the dashboard";
pub const TRACK_FAILED: &str = "⚠️ Couldn't play";
pub const TRACK_FAILED_TRACKS: &str = "tracks";
pub const TRACK_FAILED_MORE: &str = "more";
pub const TRACK_UNTITLED: &str = "(untitled)";
pub const PROGRESS_ENDS: &str = "ends";
pub const PROGRESS_STARTED: &str = "Started";
pub const PROGRESS_PAUSED_AT: &str = "Paused at";
pub const PROGRESS_PAUSED: &str = "Paused";
/// After the length of a track on repeat, which has no end time to show.
pub const PROGRESS_ON_REPEAT: &str = "on repeat";
/// A track on repeat whose length is unknown.
pub const PROGRESS_REPEATING: &str = "On repeat";
pub const TRACK_FAILED_OPEN: &str = "couldn't open the stream";
pub const TRACK_FAILED_FORMAT: &str = "that format isn't supported";
pub const TRACK_FAILED_BROKE_OFF: &str = "the stream broke off partway";
pub const TRACK_FAILED_SEEK: &str = "couldn't seek in it";
/// `/seek`'s own words for an empty queue (not `FAIL_NOTHING_PLAYING`).
pub const FAIL_NO_TRACK_PLAYING: &str = "No track playing";
pub const OP_TRACK_ABSENT: &str = "That track is no longer queued";
pub const OP_TRACK_PLAYING: &str = "That track is playing";
pub const BUTTON_OUT_OF_DATE: &str = "This button is out of date.";
pub const NP_BUTTONS_DISABLED: &str = "The now-playing buttons are turned off in this server.";
pub const OP_TRACK_STALE: &str = "That track is no longer playing";
pub const FAIL_EMPTY_VECTOR: &str = "⚠️ Empty vector not allowed!";
pub const FAIL_INSERT: &str = "⚠️ Failed to insert!";
pub const FAIL_INVALID_TOPGG_TOKEN: &str = "⚠️ Invalid top.gg token!";
pub const FAIL_INVALID_PERMS: &str = "⚠️ Invalid permissions!!";
pub const FAIL_MISSING_VOICE_PERMS: &str = "❌ I can't play in";
pub const FAIL_MISSING_TEXT_PERMS: &str = "⚠️ I can't post in";
pub const MISSING_PERMS_FIX: &str = "Ask an admin to grant it, then try again.";
pub const FAIL_MINUTES_PARSING: &str = "⚠️ Invalid formatting for 'minutes'";
pub const FAIL_NO_SONG_ON_INDEX: &str = "⚠️ There is no queued song on that index!";
pub const FAIL_NO_SONGBIRD: &str = "⚠️ Failed to get songbird!";
pub const FAIL_NO_VIRUSTOTAL_API_KEY: &str =
    "⚠️ The VIRUS_TOTAL_API_KEY environment variable is not set!";
pub const FAIL_NO_VOICE_CONNECTION: &str = "⚠️ I'm not connected to any voice channel!";
pub const FAIL_NO_QUERY_PROVIDED: &str = "⚠️ No query provided!";
pub const FAIL_NOT_IMPLEMENTED: &str = "⚠️ Function is not implemented!";
pub const FAIL_NOTHING_PLAYING: &str = "🔈 Nothing is playing!";
pub const FAIL_REMOVE_RANGE: &str = "⚠️ `until` needs to be higher than `index`!";
pub const FAIL_RESUME: &str = "⚠️ Failed to Resume Queue!";
pub const FAIL_SECONDS_PARSING: &str = "⚠️ Invalid formatting for 'seconds'";
pub const FAIL_TO_SET_CHANNEL_SIZE: &str = "⚠️ Failed to set channel size!";
pub const FAIL_WRONG_CHANNEL: &str = "⚠️ We are not in the same voice channel!";
pub const FAIL_PARSE_TIME: &str = "⚠️ Failed to parse time, speak English much?";
pub const FAIL_PLAYLIST_FETCH: &str = "⚠️ Failed to fetch playlist!";
pub const FAIL_INVALID_IP: &str = "⚠️ Invalid IP address!";
pub const FAIL_INVALID_PLAYLIST: &str = "⚠️ That isn't a YouTube playlist link!";
pub const FAIL_URL_NOT_ALLOWED: &str =
    "⚠️ I can only play links to public http(s) sites, not local or private addresses!";

pub const GUILD_ONLY: &str = "⚠️ This command can only be used in a server!";
pub const IDLE_ALERT: &str = "⚠️ I've been idle for a while so I'm going to hop off, set the idle timeout to change this! Also support my development and keep the bot idle in vc as long as you like!
[CrackTunes Patreon](https://patreon.com/CrackTunes)";
pub const IP_DETAILS: &str = "🌐 IP details for";
pub const JOINING: &str = "Joining";
/// 🪤 Distinct from [`JOINING`] on purpose. `/summon` into the channel the bot
/// is already sitting in is a no-op, and answering it with "Joining" asserts
/// an action that did not happen.
pub const ALREADY_HERE: &str = "Already in";
pub const KICKED: &str = "Kicked";
/// `/grab` could not deliver the DM (usually closed DMs).
pub const GRAB_DM_FAILED: &str = "Could not send you a DM. Check that your DMs are open.";
pub const GRABBED_NOTICE: &str = "📃 Sent you a DM with the current track!";
pub const LEAVING: &str = "👋 See you soon!";
pub const LOOP_DISABLED: &str = "🔁 Disabled loop!";
pub const LOOP_ENABLED: &str = "🔁 Enabled loop!";
pub const NO_AUTO_ROLE: &str = "⚠️ No auto role set for this server!";
pub const NO_CHANNEL_ID: &str = "⚠️ No GenericChannelId Found!";
pub const NO_DATABASE_POOL: &str = "⚠️ No Database Pool Found!";
pub const NO_GUILD_CACHED: &str = "⚠️ No Cached Guild Found!";
pub const NO_GUILD_ID: &str = "⚠️ No GuildId Found!";
pub const NO_GUILD_SETTINGS: &str = "⚠️ No GuildSettings Found!";
pub const NO_USER_AUTOPLAY: &str = "(auto)";
pub const NO_METADATA: &str = "⚠️ No metadata found!";
pub const NO_PLAY_LOGGED: &str = "⚠️ This track is not in the play log, so it can't be downvoted.";
pub const NOT_IN_MUSIC_CHANNEL: &str = "⚠️ You are not in the music channel! Use";
pub const ONETWOFT: &str = "https://12ft.io/";
pub const OWNERS_ONLY: &str = "⚠️ This command can only be used by bot owners!";
pub const PAGINATION_COMPLETE: &str =
    "🔚 Dynamic message timed out! Run the command again to see updates.";
pub const PASSWORD_PWNED: &str = "⚠️ This password has been pwned!";
pub const PASSWORD_SAFE: &str = "🔒 This password is safe!";
pub const PAUSED: &str = "⏸️ Paused!";
pub const PLAYLIST_CREATED: &str = "📃 Created playlist!";
pub const PLAYLIST_DELETED: &str = "❌ Deleted playlist!";
pub const PLAYLIST_ADD: &str = "📃 Added to playlist!";
pub const PLAYLIST_REMOVE: &str = "❌ Removed from playlist!";
pub const PLAYLIST_LIST_EMPTY: &str = "📃 You have no playlists currently.";
pub const PLAYLIST_EMPTY: &str = "📃 This playlist has no songs!";
pub const PLAYLISTS: &str = "Playlists";
pub const PLAY_FAILED_BLOCKED_DOMAIN: &str =
    "**is either not allowed in this server or is not supported!** \n\nTo explicitely allow this domain, ask a moderator to run the `/managesources` command. [Click to see a list of supported sources.](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md)";
pub const PLAY_ALL_FAILED: &str =
    "⚠️ Cannot fetch playlist via keywords! Try passing this command an URL.";
pub const PLAY_PLAYLIST: &str = "📃 Added playlist to queue!";
pub const PLAY_SEARCH: &str = "🔎 Searching...";
pub const PLAY_QUEUE: &str = "📃 Added to queue!";
pub const PLAY_TOP: &str = "📃 Added to top!";
pub const PLAY_LOG: &str = "🎵 Last Played Songs";
pub const PREFIXES: &str = "Prefixes";
pub const PREMIUM: &str = "👑 Premium status:";
pub const PREMIUM_GRANTED: &str = "👑 Premium is on for";
pub const PREMIUM_REVOKED: &str = "Premium is off for";
pub const PREMIUM_FREE_PLAN: &str =
    "This server is on the free plan: queue history goes back 24 hours.";

pub const STATUS_TITLE: &str = "📊 Status";
pub const STATUS_SERVER: &str = "Server";
pub const STATUS_BOT: &str = "Bot";
pub const STATUS_PLAYBACK: &str = "Playback";
pub const STATUS_SETTINGS: &str = "Settings";
pub const STATUS_PREMIUM: &str = "👑 Premium";
pub const STATUS_FREE: &str = "Free: queue history goes back 24 hours.";
pub const STATUS_IDLE: &str = "Idle: not playing here.";
pub const STATUS_GAME: &str = "🎭 A /gp game is on.";
pub const STATUS_UNTITLED: &str = "(untitled)";
pub const STATUS_MORE_QUEUED: &str = "more in the queue.";
pub const STATUS_IDLE_TIMEOUT: &str = "Idle timeout";
pub const STATUS_VOLUME: &str = "Volume";
pub const STATUS_AUTOPAUSE: &str = "Autopause";
pub const STATUS_AUTOPLAY: &str = "Autoplay";
pub const STATUS_NEVER_PREMIUM: &str = "never (premium)";
pub const STATUS_ON: &str = "on";
pub const STATUS_OFF: &str = "off";
pub const PREMIUM_BAD_SERVER_ID: &str =
    "⚠️ That isn't a server id: it's a long number, like `1267282599064244255`.";
pub const PREMIUM_NOT_IN_SERVER: &str = "⚠️ I'm not in a server with that id.";
pub const PREMIUM_FAILED: &str =
    "⚠️ Couldn't change premium just now. Nothing was changed; try again in a moment.";
pub const PREMIUM_PLUG: &str = "👑 Like the bot? Support my development and unlock premium features by subscribing to my Patreon!
[CrackTunes Patreon](https://patreon.com/CrackTunes)";
pub const PREMIUM_THANKS: &str = "👑 Thanks for supporting CrackTunes! Your server has premium.";
pub const PROGRESS: &str = "Progress";
pub const PHONE_NUMBER_INFO_ERROR: &str = "⚠️ Failed to fetch phone number info!";
pub const QUEUE_EXPIRED: &str = "This command has expired.\nPlease feel free to reinvoke it!";
pub const QUEUE_IS_EMPTY: &str = "Queue is empty!";
pub const QUEUE_NO_SONGS: &str = "There's no songs up next!";
pub const QUEUE_NO_TITLE: &str = "Unknown title";
pub const QUEUE_NO_SRC: &str = "Unknown source url";
pub const QUEUE_NOTHING_IS_PLAYING: &str = "Nothing is playing!";
pub const QUEUE_NOW_PLAYING: &str = "🔊 Now playing";
pub const NOW_PLAYING_POINTER: &str = "🔊 Now playing:";
pub const STATUS_FINISHED_TITLE: &str = "⏹️ Finished";
pub const STATUS_FINISHED_DESCRIPTION: &str = "Nothing is playing. Use /play to start again.";
pub const EPHEMERAL_REPLIES_ON: &str = "🙈 /play, /skip and /nowplaying now reply privately.";
pub const EPHEMERAL_REPLIES_OFF: &str = "👀 /play, /skip and /nowplaying now reply in the channel.";
pub const CONTROL_ECHOES_ON: &str =
    "📣 Button and dashboard controls now post a line in the channel.";
pub const CONTROL_ECHOES_OFF: &str =
    "🔇 Button and dashboard controls no longer post a line in the channel.";
pub const NP_BUTTONS_ON: &str = "🎛️ The now-playing message now has buttons.";
pub const NP_BUTTONS_OFF: &str =
    "🚫 The now-playing message no longer has buttons. The commands still work.";
pub const QUEUE_PAGE_OF: &str = "of";
pub const QUEUE_PAGE: &str = "Page";
pub const QUEUE_UP_NEXT: &str = "⌛ Up next";
pub const REGISTER_OWNER_ONLY: &str = "Can only be used by bot owner";
pub const REMOVED_QUEUE_MULTIPLE: &str = "❌ Removed multiple tracks from queue!";
pub const REMOVED_QUEUE: &str = "❌ Removed from queue";
pub const RESUMED: &str = "▶ Resumed!";
pub const REQUESTED_BY: &str = "Requested by";
pub const ROLE_CREATED: &str = "📝 Created role!";
pub const ROLE_DELETED: &str = "🗑️ Deleted role!";
pub const ROLE_NOT_FOUND: &str = "⚠️ Role not found!";
pub const SCAN_QUEUED: &str = "🔍 Scan queued! Use";
pub const SEARCHING: &str = "🔎 Searching...";
/// `/search`'s pick-a-result menu could not be posted in the channel.
pub const SEARCH_RESULTS_NOT_POSTED: &str = "⚠️ Could not post the search results here.";
pub const SEEKED: &str = "⏩ Seeked current track to";
pub const SEEK_FAIL: &str = "❌⏩ Failed to seek to";
pub const SHUFFLED_SUCCESS: &str = "🔀 Shuffled successfully!";
pub const SKIP_VOTE_EMOJI: &str = "🗳";
pub const SKIP_VOTE_MISSING: &str = "more vote(s) needed to skip!";
pub const SKIP_VOTE_USER: &str = "has voted to skip!";
pub const SKIPPED_ALL: &str = "⏭️ Skipped until infinity!";
pub const SKIPPED_TO: &str = "⏭️ Skipped to";
pub const SKIPPED: &str = "⏭️ Skipped!";
pub const DOWNVOTED_AND_SKIPPED: &str =
    "👎 Downvoted and skipped. Its latest play is hidden from /playlog.";
pub const SKIPPED_NOT_LOGGED: &str =
    "⏭️ Skipped. This track isn't in the play log, so there was nothing to hide from /playlog.";
pub const MOVED_ON_NOT_SKIPPED: &str =
    "⚠️ The track had already moved on, so nothing was skipped or downvoted.";
pub const DOWNVOTED_BUT_MOVED_ON: &str =
    "👎 Downvoted. The track had already moved on, so nothing was skipped.";
// The old copy here asked "Did you forget to provide your Spotify application's
// client ID and secret?" -- advice that stopped being actionable. Spotify has
// blocked the creation of new Web API apps since roughly December 2025, so an
// operator without credentials often CANNOT obtain them, and sending them to the
// dashboard wastes their time. Tell the person in the channel what they can
// actually do instead; the operator gets the real diagnosis in the logs.
// Nothing raises this today: link resolution moved to sleevenote, and autoplay
// moved to crack-musicreco. The Display impl is what shows if
// CrackedError::SpotifyAuth ever surfaces -- so it must not claim links are
// broken, because they are not.
pub const SPOTIFY_AUTH_FAILED: &str = "⚠️ **I can't pick a next track right now.**\nThat needs Spotify recommendations, which aren't available. Queue something up and I'll keep playing.";
/// Posted when autoplay switches itself off. Nothing more: the reason is in the
/// logs, and what the listener can do about it is the same either way.
pub const AUTOPLAY_STOPPED: &str = "Autoplay off";
/// Posted instead when this deployment has no recommender at all
/// (`Data::musicreco` is `None`) -- the one reason worth saying out loud.
pub const AUTOPLAY_NEEDS_MUSICRECO: &str = "Autoplay needs crack-musicreco!";
// Operator-facing. Never sent to a channel -- this is the detail a person
// running the bot needs, and the detail a Discord user cannot act on.
// ⚠️ These describe AUTOPLAY ONLY. Spotify *links* no longer need credentials
// -- they resolve through sleevenote (`sources::sleevenote`). This message used
// to say "Spotify links and autoplay will not work", which stopped being true
// the moment link resolution moved, and sent an operator hunting for
// credentials that cannot be obtained and would not have fixed links anyway.
pub const SPOTIFY_DISABLED_LOG: &str = "spotify: autoplay DISABLED -- no client credentials, so recommendations are unavailable. Spotify LINKS are unaffected and resolve through sleevenote. Spotify has blocked new Web API app creation since ~2025-12, so this is probably not fixable by supplying credentials.";
pub const SPOTIFY_ENABLED_LOG: &str = "spotify: autoplay enabled -- client credentials accepted";
// The question an operator actually needs answered at boot: can this bot
// resolve a Spotify link? That is now about sleevenote, not about credentials.
pub const SLEEVENOTE_CONFIGURED_LOG: &str =
    "sleevenote: configured -- Spotify links resolve through";
pub const SLEEVENOTE_UNCONFIGURED_LOG: &str = "sleevenote: SLEEVENOTE_BASE_URL is not set, so Spotify links will be tried against the default and will fail unless sleevenote runs beside this bot. Set it to your sleevenote deployment.";
/// Tail of the line `/play` adds when sleevenote served a partial listing.
///
/// Says what happened without implying the bot failed: the tracks that were
/// recovered are queued and playing, and this is the footnote.
pub const SPOTIFY_LISTING_SHORT: &str = "couldn't be recovered";
pub const SPOTIFY_INVALID_QUERY: &str =
    "⚠️ **Could not find any tracks with that link!**\nAre you sure that is a valid Spotify URL?";
// Spotify resolution through sleevenote (`sources::sleevenote`). These stay
// separate for the same reason the client's error variants do: "this does not
// exist", "the service is down" and "our scraper broke" call for three
// different reactions, and one of the three is not the user's to have. A
// Spotify link used to fail with a yt-dlp error about a URL yt-dlp was never
// going to be able to fetch, which told the reader nothing true.
pub const SPOTIFY_NOT_CONFIGURED: &str = "⚠️ **Spotify support isn't set up on this bot.**\nI can still play it — try searching for the track or artist by name.";
pub const SPOTIFY_UNREACHABLE: &str = "⚠️ **I can't reach Spotify lookup right now.**\nTry again in a moment, or search for the track by name.";
pub const SPOTIFY_NOT_FOUND: &str = "⚠️ **Spotify has nothing at that link.**\nEither the link is wrong, or it is private or personalized — a playlist Spotify generates for you, like your daylist, cannot be fetched by anyone else. Search for the track by name and I'll play it.";
pub const SPOTIFY_TIMEOUT: &str = "⏳ **Spotify took too long to answer.**\nTry again in a moment.";
// Deliberately offers no retry: the lookup service stopped matching Spotify's
// page, so retrying the same link fails identically until it is fixed.
pub const SPOTIFY_LOOKUP_BROKEN: &str = "⚠️ **Spotify lookup is broken right now.**\nThis has been logged. Search for the track by name and I'll play it.";
/// `overloaded`: every browser context was busy, so the lookup was shed.
///
/// Nothing was attempted, so this must not read as a failure of the link or of
/// Spotify. It clears on its own in seconds, which is why it says to retry
/// rather than sending the reader off to search by name.
pub const SPOTIFY_LOOKUP_BUSY: &str = "⏳ **Spotify lookup is at capacity right now.**\nNothing was lost — send the same link again in a few seconds.";
/// `browser_unavailable`: the lookup service is restarting its browser.
///
/// Transient like [`SPOTIFY_LOOKUP_BUSY`] and kept separate for the same
/// reason sleevenote keeps the codes apart: busy and restarting are different
/// facts, and only one of them means the service is coming back up.
pub const SPOTIFY_LOOKUP_RESTARTING: &str = "⏳ **Spotify lookup is restarting.**\nIt'll be back in a few seconds — send the same link again then.";
/// `listing_empty`: the listing is real and has nothing in it we can see.
///
/// Deliberately NOT [`SPOTIFY_LOOKUP_BROKEN`]. The two used to be the same
/// arm, so a daylist -- which Spotify serves empty to anyone who is not its
/// signed-in owner -- told the reader the whole integration was down. It
/// offers no retry, because retrying the same link cannot work, and names the
/// one thing that does: the share link, measured to resolve normally when the
/// personalised link does not.
pub const SPOTIFY_LISTING_EMPTY: &str = "⚠️ **That link looks empty from here.**
Spotify only shows its contents to you — personalized lists like your daylist look empty to anyone else, including me. Open it on Spotify, tap share, and paste *that* link: it usually works.";
/// `extraction_silent` that survived the client's retry. Unlike
/// [`SPOTIFY_LOOKUP_BROKEN`] this one IS worth retrying -- measured at ~14%
/// per attempt with every failing id succeeding later -- so the message says
/// so instead of sending the user off to search by name.
pub const SPOTIFY_LOOKUP_FLAKY: &str = "⚠️ **Spotify didn't answer that one.**\nThis usually clears on a second try — run the same link again.";
// Distinct from the above, and the distinction is the whole reason sleevenote
// keeps these two apart: something *was* recovered, just not all of it. The
// service returns the shortfall rather than a partial listing, so there is
// nothing to queue -- but "broken" overstates it, and the reader has a real
// alternative rather than nothing to try.
pub const SPOTIFY_PARTIAL_LISTING: &str = "⚠️ **Spotify only gave me part of that list.**\nI won't queue half a playlist, so nothing was added. Albums and individual tracks resolve reliably; large editorial playlists often don't.";
pub const SPOTIFY_LOOKUP_FAILED: &str =
    "⚠️ **Spotify lookup failed.**\nSearch for the track by name and I'll play it.";
// The wire call succeeded and the answer was empty -- a podcast-only playlist,
// or one that is nothing but local files. A success that plays nothing still
// needs to say so.
pub const SPOTIFY_NOTHING_PLAYABLE: &str = "⚠️ **Nothing playable at that link.**\nIt has no songs I can find — local files and podcast episodes don't work.";
// `/gp submit` takes exactly one song, and which song it is *is* the game, so
// silently taking track one of an album would replace the player's choice with
// ours.
pub const SPOTIFY_GP_ONE_SONG: &str = "⚠️ **That link is a whole album or playlist, and I need one song.**\nOpen it on Spotify, pick the track you want, and link that one.";
pub const SONG_MOVED: &str = "🔀 Moved song";
pub const SONG_MOVED_FROM: &str = "from index";
pub const SONG_MOVED_TO: &str = "to index";
pub const STOPPED: &str = "⏹️ Stopped!";
pub const SUGGESTION: &str = "📝 Suggestion";
pub const SUBCOMMAND_NOT_FOUND: &str = "⚠️ Subcommand {subcommand} for group {group} not found!";
pub const TIMEOUT: &str = "⏱️ User Timed Out!";
pub const TRACK_DURATION: &str = "Track duration:";
pub const TRACK_NOT_FOUND: &str = "⚠️ **Could not play track!**\nYour request yielded no results.";
pub const TRACK_INAPPROPRIATE: &str = "⚠️ **Could not play track!**\nThe video you requested may be inappropriate for some users, so sign-in is required.";
pub const TRACK_TIME_TO_PLAY: &str = "Estimated time until play:";
pub const TEST: &str = "🔧 Test";
pub const TEXT_CHANNEL_CREATED: &str = "📝 Created text channel!";
pub const CATEGORY_CREATED: &str = "📝 Created category!";
pub const UNTIL: &str = "Until";
pub const UNKNOWN: &str = "Unknown";
pub const UNAUTHORIZED_USER: &str = "⚠️ You are not authorized to use this command!";
pub const UNKNOWN_LIT: &str = UNKNOWN;
pub const WAYBACK_SNAPSHOT: &str = "Wayback snapshot for";
pub const VERSION_LATEST: &str = "Find the latest version [here]";
pub const VERSION: &str = "Version";
pub const VERSION_LATEST_HASH: &str = "Build hash [here]";
pub const VOLUME: &str = "🔊 Volume";
pub const OLD_VOLUME: &str = "Old Volume";
pub const VOICE_CHANNEL_CREATED: &str = "🔊 Created voice channel!";

pub const VOTE_TOPGG_TEXT: &str = "✅ Vote for CrackTunes on";
pub const VOTE_TOPGG_LINK_TEXT: &str = "top.gg!";
pub const VOTE_TOPGG_LINK_TEXT_SHORT: &str = "vote";
pub const VOTE_TOPGG_URL: &str = "https://top.gg/bot/1115229568006103122/vote";
pub const VOTE_TOPGG_VOTED: &str = "Thank you for voting within the last 12 hours! Remember to vote again to get free premium features and support the bot.";
pub const VOTE_TOPGG_NOT_VOTED: &str = "You haven't voted recently! Here is the link to vote :)";

pub const INVITE_TEXT: &str = "🔗 ";
pub const INVITE_LINK_TEXT: &str = "Invite Crack Tunes to your server!";
pub const INVITE_LINK_TEXT_SHORT: &str = "invite";
pub const INVITE_URL: &str = "https://discord.com/oauth2/authorize?client_id=1115229568006103122&permissions=551940115520&scope=bot+applications.commands";

// ---- `/gp` party game (`commands::music::gp`) ----
pub const GP_TITLE: &str = "🎭 What's Your Song?";
pub const GP_RULES_TEXT: &str = "The host picks where the prompts come from: one category, several ticked from a menu, or 🎲 Random (the default), which takes each round's prompt from a different category. Each round the bot posts a prompt and everyone in the voice channel secretly submits a song for it. The songs then play one by one: guess whose is whose, and 👍 the ones you like. Submitters are revealed at the end of each round (or, with `reveal:song`, as each song ends), and every round ends with its results: each song, whose it was, who got it, and the scores.";
pub const GP_HOW_TO_TITLE: &str = "How to play";
pub const GP_HOW_TO: &str = "• `/gp start [category] [rounds] [timer] [reveal]` — start in your voice channel (default 5 rounds, 3 minutes to submit; `reveal:song` names each submitter as their song ends instead of at the end of the round; `results:false` skips the round-results embed)\n• `/gp submit <song>` — your song for the current prompt; only you see the reply; submitting again replaces it\n• The window closes on the timer, as soon as everyone in the voice channel is in, or when the host runs `/gp close`\n• While a song plays: pick who submitted it from the dropdown (you can change your pick until it ends) and tap 👍 if you like it\n• `/gp skip` — host ends the current song · `/gp voteskip` — a majority ends it, or pulls it outright if it is your own (votes are anonymous) · `/gp votefull` — a majority hears it in full · `/gp status` — where things stand · `/gp end` — abort\n• You need to be in the game's voice channel, and to have submitted a song, to guess, 👍 or vote\n\n**Scoring:** +100 for a correct guess · +100 if nobody guesses you · +10 for every 👍 your song gets · +50 if the room votes to hear your song in full. Up to 25 people per round.\n\nBy default each song plays as a 45-second clip starting 30 seconds in, so a round moves; `/gp start ... clips:false` plays them whole, and `clip_start`/`clip_length` tune the clip when clips are on.";
pub const GP_PICK_TITLE: &str = "☑️ Pick the categories";
pub const GP_PICK_TEXT: &str = "Tick the categories to draw prompts from, then press **Start**. Each round's prompt comes from a different one of them.";
pub const GP_PICK_PLACEHOLDER: &str = "Categories to draw prompts from";
pub const GP_PICK_START: &str = "Start";
pub const GP_PICK_CANCEL: &str = "Cancel";
pub const GP_PICK_CHOSEN: &str = "✅ Starting with";
pub const GP_PICK_CANCELLED: &str = "Cancelled — no game started.";
pub const GP_PICK_TIMED_OUT: &str = "⏳ Nobody pressed Start, so no game was started.";
pub const GP_PICK_NOT_HOST: &str = "🙅 Only whoever ran `/gp start` can pick the categories.";
pub const GP_STARTED: &str = "🎭 Game on!";
pub const GP_STARTED_ROUNDS: &str = "rounds of";
pub const GP_STARTED_TIMER: &str = "to submit each round.";
pub const GP_QUEUE_CLEARED: &str = "(cleared the existing queue)";
pub const GP_STARTED_CLIP: &str = "Playing";
pub const GP_STARTED_CLIP_FROM: &str = "of each song, from";
pub const GP_ROUND_TITLE: &str = "🎭 Round";
pub const GP_SONG_TITLE: &str = "Song";
pub const GP_PROMPT_HOW_TO_TITLE: &str = "How";
pub const GP_PROMPT_HOW_TO: &str =
    "`/gp submit <song>` — one song each, only you see the reply, submitting again replaces it.";
pub const GP_PROMPT_CLOSES_TITLE: &str = "Submissions close";
pub const GP_PROMPT_CLOSES_EARLY: &str =
    "— or as soon as everyone in the voice channel has submitted.";
pub const GP_WINDOW_WARNING: &str = "⏳ 30 seconds left!";
pub const GP_WINDOW_WARNING_IN: &str = "in so far, closing";
pub const GP_WINDOW_CLOSED: &str = "🔒 Submissions closed —";
pub const GP_WINDOW_CLOSED_SONGS: &str = "song(s). Playing now!";
pub const GP_WINDOW_EMPTY: &str = "🔒 Submissions closed — nobody submitted, skipping this one.";
pub const GP_SUBMITTED: &str = "🤫 Got it:";
pub const GP_SUBMITTED_REPLACED: &str = "🔁 Swapped your song for:";
pub const GP_SUBMITTED_OF: &str = "in";
pub const GP_CLOSED_BY_HOST: &str = "🔒 Submissions closed by the host —";
pub const GP_ROUND_HINT: &str =
    "Whose song is this? Pick a name below — you can change your pick until it ends.";
pub const GP_LIKE_HINT: &str = "Tap 👍 if you like it (+10 to whoever submitted it).";
pub const GP_SELECT_PLACEHOLDER: &str = "Whose song is this?";
pub const GP_LIKE_LABEL: &str = "Like";
pub const GP_LIKED: &str = "👍 Liked";
pub const GP_UNLIKED: &str = "👍 Like removed";
pub const GP_LIKES: &str = "👍 Likes";
pub const GP_GUESS_RECORDED: &str = "🤫 Guess locked in. You can change it until the song ends.";
pub const GP_GUESS_CHANGED: &str = "🔁 Guess changed.";
pub const GP_REVEAL: &str = "🎉 It was";
pub const GP_GUESSED_RIGHT: &str = "Guessed right";
pub const GP_NOBODY_GUESSED: &str = "nobody!";
pub const GP_FOOLED_EVERYONE: &str = "🃏 Fooled everyone";
pub const GP_SCOREBOARD: &str = "🏆 Scoreboard";
pub const GP_RESUMED: &str =
    "🔁 **We're back.** The bot restarted mid-game, and the game picks up where it was:";
pub const GP_RESUMED_WINDOW: &str = "submissions are still open for round";
pub const GP_RESUMED_WINDOW_CLOSED: &str =
    "the submission window for round {round} closed while I was away, so it closes now.";
pub const GP_RESUMED_SONG: &str = "the song that was playing starts again. Guess and 👍 it again — nothing cast before the restart counts.";
pub const GP_LOST: &str = "💤 A game was running when the bot went down and could not be picked up again. Here is the scoreboard as it stood.";
pub const GP_GAME_OVER: &str = "🏆 Game over — final scores";
pub const GP_ROUND_SKIPPED: &str = "⏭️ Song skipped.";
pub const GP_TRACK_FAILED: &str = "⚠️ Couldn't play this one";
pub const GP_TRACK_FAILED_NOTE: &str =
    "The song wouldn't stream, so it was skipped. Nobody scored for it.";
pub const GP_ENDED_BY: &str = "🛑 Game ended by";
pub const GP_STATUS_SUBMITTING: &str = "🎭 Submissions open — round";
pub const GP_STATUS_PLAYING: &str = "🎭 Round";
pub const GP_STATUS_PROMPT: &str = "Prompt";
pub const GP_STATUS_CLOSES: &str = "Closes";
pub const GP_STATUS_SUBMITTED: &str = "Submitted";
pub const GP_STATUS_GUESSED: &str = "Guessed this song";
pub const GP_STATUS_LIKES: &str = "👍 Likes";
pub const GP_STATUS_SCORES: &str = "Scores";
pub const GP_NOBODY_YET: &str = "nobody yet";
pub const GP_SUBMITTED_IN_VC: &str = "in the voice channel";
pub const GP_VOTESKIP_COUNTED: &str = "🗳️ Vote counted —";
pub const GP_VOTESKIP_SO_FAR: &str = "so far,";
pub const GP_VOTESKIP_NEEDED: &str = "more to skip this song.";
pub const GP_VOTESKIP_CARRIED: &str = "🗳️ Vote counted — that was the majority. Skipping it.";
pub const GP_VOTESKIP_ROOM: &str = "🗳️ Someone voted to skip this song —";
pub const GP_VOTESKIP_ROOM_NEEDED: &str = "more and it's gone.";
pub const GP_VOTESKIP_PASSED: &str = "🗳️ The room voted to skip this one.";
pub const GP_VOTESKIP_OWN: &str = "🙈 Your song, your call — pulled it.";
pub const GP_VOTESKIP_PULLED: &str = "🙈 Whoever submitted this one pulled it.";
pub const GP_VOTEFULL_COUNTED: &str = "🎶 Vote counted —";
pub const GP_VOTEFULL_NEEDED: &str = "more to hear the whole thing.";
pub const GP_VOTEFULL_CARRIED: &str = "🎶 Vote counted — that was the majority. It runs on.";
pub const GP_VOTEFULL_ROOM: &str = "🎶 Someone wants to hear the whole thing —";
pub const GP_VOTEFULL_ROOM_NEEDED: &str = "more and it runs on.";
pub const GP_VOTEFULL_PASSED: &str = "🎶 The room wants all of it — letting it run.";
pub const GP_VOTEFULL_ALREADY: &str = "🎶 Already running in full.";
pub const GP_REVEAL_HELD: &str = "🤐 Whose song? All revealed at the end of the round.";
pub const GP_RESULTS_TITLE: &str = "— results";
pub const GP_RESULTS_GUESSED_BY: &str = "guessed by";
pub const GP_RESULTS_GUESSED_COUNT: &str = "guessed right";
pub const GP_RESULTS_THIS_ROUND: &str = "This round";
pub const GP_RESULTS_NOBODY_SCORED: &str = "nobody scored";
pub const GP_STARTED_REVEAL_SONG: &str = "Submitters are revealed after each song.";
pub const GP_STARTED_NO_RESULTS: &str = "No round results.";
pub const GP_FULL_SONG: &str = "🎶 Played in full";
pub const GP_FULL_SONG_NOTE: &str =
    "The room voted to hear the whole thing (+50 to whoever submitted it).";
pub const GP_ABORTED: &str =
    "🎭 The game could not post to this channel, so it was ended. `/gp start` to try again.";

pub const FAIL_GP_ALREADY_RUNNING: &str =
    "🎭 A game is already running here. `/gp status` to see it, `/gp end` to abort.";
pub const FAIL_GP_NO_GAME: &str = "🎭 No game is running. `/gp start` to begin one.";
pub const FAIL_GP_NOT_PLAYING: &str = "🎭 No song is playing right now.";
pub const FAIL_GP_OWNS_PLAYBACK: &str = "🎭 A game is running; that command would break the rounds. `/gp skip`, `/gp close` or `/gp end` instead.";
pub const FAIL_GP_NOT_HOST: &str = "🎭 Only the game's host can do that.";
pub const FAIL_GP_NOT_IN_GAME_VC: &str = "🎭 You need to be in the game's voice channel for that.";
pub const FAIL_GP_TOO_MANY: &str = "🎭 That's the maximum number of players this round:";
pub const FAIL_GP_STALE_ROUND: &str = "🎭 That song is over.";
pub const FAIL_GP_NOT_A_PLAYER: &str = "🎭 That person hasn't submitted a song this round.";
pub const FAIL_GP_WINDOW_CLOSED: &str =
    "🎭 Submissions aren't open right now — wait for the next prompt.";
pub const FAIL_GP_OWN_SONG: &str = "🎭 Nice try — you can't 👍 your own song.";
pub const FAIL_GP_NOT_GUESSABLE: &str =
    "🎭 Only one song this round, so there's nothing to guess — you can still 👍 it.";
pub const FAIL_GP_ALREADY_VOTED: &str = "🎭 You've already voted to skip this song.";
pub const FAIL_GP_ALREADY_VOTED_FULL: &str = "🎭 You've already voted to hear this one in full.";
pub const FAIL_GP_OWN_SONG_FULL: &str =
    "🎭 Nice try — you can't vote your own song up to its full length.";
pub const FAIL_GP_NOT_CLIPS: &str =
    "🎭 This game is playing whole songs already, so there's nothing to vote up.";
pub const FAIL_GP_NOT_A_GAME_PLAYER: &str =
    "🎭 You're not in this game — `/gp submit <song>` on the next prompt to join in.";

pub const AUDITLOG_TITLE: &str = "📜 Queue history";
pub const AUDITLOG_NO_DATABASE: &str =
    "📜 The queue history needs a database, and this bot is running without one.";
pub const AUDITLOG_BAD_SINCE: &str =
    "⚠️ `since` takes a number and a unit: `90m`, `6h`, `2d` or `1w` (at most 52w).";
pub const AUDITLOG_FAILED: &str =
    "⚠️ Couldn't read the queue history just now. Try again in a moment.";
pub const AUDITLOG_EMPTY: &str = "📜 Nothing in the queue history matches that.";
pub const AUDITLOG_TRUNCATED: &str = "_Showing the newest 200. Narrow it with `since` or `user`._";
pub const AUDITLOG_GP_HIDDEN: &str =
    "_Entries from the `/gp` game in progress are hidden until it ends._";
pub const AUDITLOG_ONLY_OLDER: &str =
    "📜 Nothing in the queue history matches that in the last 24 hours.";
/// Shown where the free 24-hour window hid older history: in `/auditlog`
/// and on the dashboard's history page.
pub const PREMIUM_HISTORY: &str = "Older history is a premium feature.";
/// Shown on the dashboard's queue page to free servers, under the disabled
/// controls.
pub const PREMIUM_CONTROLS: &str = "Dashboard controls are a premium feature.";
/// Shown on the dashboard's queue page to a premium server's member who is
/// not in the bot's voice channel, under the disabled controls.
pub const JOIN_VOICE_FOR_CONTROLS: &str = "Join the bot's voice channel to use the controls.";
pub const PATREON_URL: &str = "https://patreon.com/CrackTunes";
// The dashboard's signed-out landing page, which is also its link preview
// (#589).
pub const DASH_LANDING_TITLE: &str = "CrackTunes dashboard";
pub const DASH_LANDING_DESCRIPTION: &str =
    "See what's playing in your Discord server and control the queue from your browser.";
pub const DASH_SIGN_IN: &str = "Sign in with Discord";

// The now-playing buttons: symbols only, so the row never wraps unevenly on
// a narrow screen (#588). Fully qualified (with U+FE0F where a symbol has a
// text form), which is what Discord expects of a button emoji.
pub const NP_BUTTON_PAUSE: &str = "⏸️";
pub const NP_BUTTON_RESUME: &str = "▶️";
pub const NP_BUTTON_SKIP: &str = "⏭️";
pub const NP_BUTTON_REPEAT: &str = "🔁";
pub const NP_BUTTON_SHUFFLE: &str = "🔀";
pub const QUEUE_RESUMED: &str = "♻️ Back after a restart — picking up where we left off.";

pub const SEARCH_TOO_SHORT: &str = "Type at least two characters to search.";
pub const SEARCH_TOO_LONG: &str = "That search is too long (100 characters at most).";
pub const SEARCH_UNREADABLE: &str = "The query string could not be read.";
pub const SEARCH_CONTROL: &str = "That search has characters it cannot use.";
pub const SEARCH_NO_SOURCES: &str = "Pick at least one place to search.";
pub const SEARCH_UNKNOWN_SOURCE: &str = "That is not a place the dashboard can search.";

// Voice rooms
/// Used when a rendered room name would otherwise be empty.
pub const ROOM_NAME_FALLBACK: &str = "Room";

// The room panel. It is the room's documentation: every control is named in
// plain words with the slash command that does the same thing.
pub const ROOM_PANEL_TITLE_PREFIX: &str = "🔊";
pub const ROOM_FIELD_OWNER: &str = "Owner";
pub const ROOM_FIELD_HELPERS: &str = "Helpers";
pub const ROOM_FIELD_ACCESS: &str = "Locked / chat";
pub const ROOM_FIELD_LIMIT: &str = "Limit";
pub const ROOM_FIELD_REFERENCE: &str = "What the buttons do";
pub const ROOM_NO_HELPERS: &str = "None";
pub const ROOM_NO_LIMIT: &str = "No limit";
pub const ROOM_LIMIT_PEOPLE: &str = "people";
pub const ROOM_STATE_OPEN: &str = "🔓 Open";
pub const ROOM_STATE_LOCKED: &str = "🔒 Locked";
pub const ROOM_STATE_CHAT_OPEN: &str = "💬 Chat open";
pub const ROOM_STATE_CHAT_LOCKED: &str = "🤐 Chat locked";

// Button labels: an emoji and a word, never an emoji alone.
pub const ROOM_BTN_KICK: &str = "🚫 Kick";
pub const ROOM_BTN_MUTE: &str = "🔇 Mute";
pub const ROOM_BTN_UNMUTE: &str = "🔊 Unmute";
pub const ROOM_BTN_LET_IN: &str = "↩️ Let back in";
pub const ROOM_BTN_LOCK: &str = "🔒 Lock";
pub const ROOM_BTN_UNLOCK: &str = "🔓 Unlock";
pub const ROOM_BTN_CHAT_LOCK: &str = "🤐 Lock chat";
pub const ROOM_BTN_CHAT_UNLOCK: &str = "💬 Unlock chat";
pub const ROOM_BTN_LIMIT: &str = "👥 Limit";
pub const ROOM_BTN_RENAME: &str = "✏️ Rename";
pub const ROOM_BTN_TRANSFER: &str = "🔑 Transfer";
pub const ROOM_BTN_HELPERS: &str = "🛡️ Helpers";
pub const ROOM_BTN_WHO_CAN_JOIN: &str = "🚪 Who can join";
pub const ROOM_BTN_CLAIM: &str = "👑 Claim";
pub const ROOM_BTN_HELP: &str = "❓ Help";

// One reference line per control, name then what it does.
pub const ROOM_REF_KICK: &str =
    "**Kick** · `/room kick` — disconnect someone and keep them out; asks for a reason.";
pub const ROOM_REF_MUTE: &str =
    "**Mute** · `/room mute` — stop someone from speaking here; the reason is optional.";
pub const ROOM_REF_UNMUTE: &str = "**Unmute** · `/room unmute` — let a muted person speak again.";
pub const ROOM_REF_LET_IN: &str =
    "**Let back in** · `/room letin` — lift a kick so the person can rejoin.";
pub const ROOM_REF_LOCK: &str =
    "**Lock / Unlock** · `/room lock` · `/room unlock` — close the room to newcomers, or open it again.";
pub const ROOM_REF_CHAT_LOCK: &str =
    "**Lock chat / Unlock chat** · `/room chatlock` · `/room chatunlock` — stop or allow typing in this room's chat.";
pub const ROOM_REF_LIMIT: &str =
    "**Limit** · `/room limit` — cap how many people fit (0 means no limit).";
pub const ROOM_REF_RENAME: &str = "**Rename** · `/room rename` — change the room's name.";
pub const ROOM_REF_TRANSFER: &str =
    "**Transfer** · `/room transfer` — hand the room to someone else in it.";
pub const ROOM_REF_HELPERS: &str =
    "**Helpers** · `/room helper add` · `/room helper remove` — let people run these controls with you.";
pub const ROOM_REF_WHO_CAN_JOIN: &str =
    "**Who can join** · `/room allow` · `/room block` · `/room unblock` — let a person or role in, or keep a role out.";
pub const ROOM_REF_CLAIM: &str =
    "**Claim** · `/room claim` — take over a room whose owner has left.";
pub const ROOM_REF_PANEL: &str = "**Panel** · `/room panel` — post this panel again.";
pub const ROOM_REF_HELP: &str = "**Help** — a private, longer guide.";
pub const ROOM_REF_DELETED: &str = "This room is deleted when everyone leaves.";
pub const ROOM_REF_NO_DEAFEN: &str =
    "Deafen isn't available: Discord can't deafen someone in just one channel.";

// The long private guide.
pub const ROOM_GUIDE_TITLE: &str = "How rooms work";
pub const ROOM_GUIDE_INTRO: &str = "Join the hub channel and CrackTunes makes you a voice room of your own. You own it, and you can share the controls with helpers. Every control is a button on the room's panel and a `/room` command; either works.";
pub const ROOM_GUIDE_WHO: &str = "The owner and helpers — and server moderators — can use these controls. If the owner has left, anyone in the room can claim it.";
pub const ROOM_GUIDE_RUN: &str =
    "Run `/room` commands from the room's voice channel or from its chat.";

// Refusals: one plain sentence each.
pub const ROOM_NOT_IN_ROOM: &str =
    "You're not in a room. Run this from your room's voice channel or its chat.";
pub const ROOM_NOT_ALLOWED: &str = "Only this room's owner or helpers can do that.";
pub const ROOM_OWNER_ONLY: &str = "Only this room's owner can do that.";
pub const ROOM_PROTECTED_TARGET: &str =
    "You can't do that to yourself, the room's owner, another helper, a moderator, or CrackTunes.";
pub const ROOM_TARGET_NOT_IN_ROOM: &str = "That person isn't in this room.";
pub const ROOM_NOT_BLOCKED: &str = "That person isn't blocked from this room.";
pub const ROOM_CANNOT_JOIN_HERE: &str =
    "This server's own permissions keep them out of this room, so there's nothing for a room control to change.";
pub const ROOM_RENAME_LIMITED: &str = "Renaming is limited to twice every 10 minutes. Try again in";
pub const ROOM_BAD_LIMIT: &str = "The limit must be a number from 0 to 99 (0 means no limit).";
pub const ROOM_BAD_NAME: &str = "A room name must be 1 to 100 characters.";
pub const ROOM_REASON_REQUIRED: &str = "Give a reason.";
pub const ROOM_REASON_TOO_LONG: &str = "That reason is too long (200 characters at most).";
pub const ROOM_MISSING_CHOICE: &str = "Pick who or what this is for, then submit again.";
pub const ROOM_OWNER_PRESENT: &str = "The owner is still in the room, so it can't be claimed.";
pub const ROOM_ALREADY_OWNER: &str = "You already own this room.";
pub const ROOM_OWNS_ANOTHER_ROOM: &str = "That member already owns a room.";
pub const ROOM_CANNOT_BLOCK_ROLE: &str = "That role can't be blocked: it would lock out the room's owner or CrackTunes. Use Lock instead.";
pub const ROOM_CANNOT_ALLOW_ROLE: &str =
    "That would let everyone in, which is what Unlock is for. Use Unlock instead.";
pub const ROOM_TOO_MANY_FOR_MENU: &str =
    "There are too many people to pick from here. Use the `/room` command instead.";
pub const ROOM_MISSING_PERMISSIONS: &str = "CrackTunes is missing a permission on this room's category. An admin can run `/rooms status` to see which.";
pub const ROOM_DISCORD_FAILED: &str = "Discord refused that. Try again in a moment.";
pub const ROOM_NO_DATABASE: &str = "Voice rooms need a database, and this CrackTunes has none.";
pub const ROOM_ROOMS_OFF: &str = "Voice rooms aren't turned on in this server.";
pub const ROOM_NOBODY_MUTED: &str = "Nobody in this room is muted.";
pub const ROOM_BUSY: &str = "This room is busy — try again in a moment.";

// The panel's modals. Discord caps a modal title and a field label at 45
// characters, and a placeholder at 100.
pub const ROOM_MODAL_KICK: &str = "Kick someone";
pub const ROOM_MODAL_MUTE: &str = "Mute someone";
pub const ROOM_MODAL_UNMUTE: &str = "Unmute someone";
pub const ROOM_MODAL_LET_IN: &str = "Let someone back in";
pub const ROOM_MODAL_LIMIT: &str = "Member limit";
pub const ROOM_MODAL_RENAME: &str = "Rename the room";
pub const ROOM_MODAL_TRANSFER: &str = "Hand the room over";
pub const ROOM_MODAL_HELPERS: &str = "Helpers";
pub const ROOM_MODAL_WHO_CAN_JOIN: &str = "Who can join";
pub const ROOM_LABEL_MEMBER: &str = "Who";
pub const ROOM_LABEL_REASON: &str = "Reason";
pub const ROOM_LABEL_REASON_OPTIONAL: &str = "Reason (optional)";
pub const ROOM_LABEL_LIMIT: &str = "How many people (0 means no limit)";
pub const ROOM_LABEL_NAME: &str = "New name";
pub const ROOM_LABEL_MODE: &str = "What to do";
pub const ROOM_LABEL_USER_OPTIONAL: &str = "Person (to let a person in)";
pub const ROOM_LABEL_ROLE_OPTIONAL: &str = "Role (for the role choices)";
pub const ROOM_PICK_MEMBER: &str = "Pick someone";
pub const ROOM_PICK_ROLE: &str = "Pick a role";
pub const ROOM_PICK_MODE: &str = "Pick one";
pub const ROOM_PLACEHOLDER_REASON: &str = "Why? Up to 200 characters.";
pub const ROOM_PLACEHOLDER_LIMIT: &str = "0 to 99";
pub const ROOM_PLACEHOLDER_NAME: &str = "Up to 100 characters";
pub const ROOM_MODE_HELPER_ADD: &str = "Add a helper";
pub const ROOM_MODE_HELPER_REMOVE: &str = "Remove a helper";
pub const ROOM_MODE_ALLOW_USER: &str = "Let a person in";
pub const ROOM_MODE_ALLOW_ROLE: &str = "Let a role in";
pub const ROOM_MODE_BLOCK_ROLE: &str = "Keep a role out";
pub const ROOM_MODE_UNBLOCK_ROLE: &str = "Stop keeping a role out";
// One line under each modal field's label; Discord caps it at 100 characters.
pub const ROOM_DESC_MEMBER: &str = "Only people this can be done to are listed.";
pub const ROOM_DESC_REASON: &str = "Required, up to 200 characters.";
pub const ROOM_DESC_REASON_OPTIONAL: &str = "Optional, up to 200 characters.";
pub const ROOM_DESC_LIMIT: &str = "A number from 0 to 99.";
pub const ROOM_DESC_NAME: &str = "1 to 100 characters.";
pub const ROOM_DESC_MODE_HELPERS: &str = "Add a helper, or remove one.";
pub const ROOM_DESC_MODE_WHO: &str = "Let a person or a role in, or keep a role out.";
pub const ROOM_DESC_HELPER: &str = "Anyone in this server.";
pub const ROOM_DESC_WHO_USER: &str = "Only for \"Let a person in\".";
pub const ROOM_DESC_WHO_ROLE: &str = "Only for the three role choices.";

// Confirmations.
pub const ROOM_DONE_KICKED: &str = "Kicked";
pub const ROOM_DONE_LET_IN: &str = "Let back in";
pub const ROOM_DONE_MUTED: &str = "Muted";
pub const ROOM_DONE_UNMUTED: &str = "Unmuted";
pub const ROOM_DONE_LOCKED: &str = "🔒 Room locked. Nobody new can join.";
pub const ROOM_DONE_UNLOCKED: &str = "🔓 Room unlocked.";
pub const ROOM_DONE_CHAT_LOCKED: &str = "🤐 Chat locked.";
pub const ROOM_DONE_CHAT_UNLOCKED: &str = "💬 Chat unlocked.";
pub const ROOM_DONE_LIMIT_SET: &str = "👥 Limit set to";
pub const ROOM_DONE_LIMIT_CLEARED: &str = "👥 Limit removed.";
pub const ROOM_DONE_RENAMED: &str = "✏️ Renamed to";
pub const ROOM_DONE_TRANSFERRED: &str = "🔑 The room now belongs to";
pub const ROOM_DONE_HELPER_ADDED: &str = "🛡️ Helper added:";
pub const ROOM_DONE_HELPER_REMOVED: &str = "🛡️ Helper removed:";
pub const ROOM_DONE_ALLOWED_USER: &str = "🚪 Allowed in:";
pub const ROOM_DONE_ALLOWED_ROLE: &str = "🚪 Allowed in:";
pub const ROOM_DONE_ROLE_BLOCKED: &str = "🚫 Kept out:";
pub const ROOM_DONE_ROLE_UNBLOCKED: &str = "🚪 No longer kept out:";
pub const ROOM_DONE_CLAIMED: &str = "👑 You now own this room.";
pub const ROOM_DONE_PANEL_POSTED: &str = "Panel posted.";

// `/rooms status` and `/rooms on|off`.
pub const ROOM_STATUS_TITLE: &str = "Voice rooms";
pub const ROOM_STATUS_ENABLED: &str = "Rooms are **on**.";
pub const ROOM_STATUS_DISABLED: &str = "Rooms are **off**.";
pub const ROOM_STATUS_HUB: &str = "Hub:";
pub const ROOM_STATUS_NO_HUB: &str = "Hub: not set";
pub const ROOM_STATUS_TEMPLATE: &str = "Name:";
pub const ROOM_STATUS_LIMIT: &str = "Default limit:";
pub const ROOM_STATUS_GRACE: &str = "Empty-room grace:";
pub const ROOM_STATUS_DASHBOARD: &str = "Dashboard settings:";
pub const ROOM_STATUS_READY: &str = "CrackTunes has everything it needs.";
pub const ROOM_STATUS_NO_HUB_YET: &str = "No hub set yet — run /rooms setup.";
pub const ROOM_STATUS_UNKNOWN: &str = "Couldn't check permissions: CrackTunes may not be able to see the hub or its category — give it View Channel there — or it's still starting up.";
pub const ROOM_FINDING_MISSING: &str = "Give CrackTunes";
pub const ROOM_FINDING_OPTIONAL: &str = "Optional: give CrackTunes";
pub const ROOM_FINDING_LOCK: &str = "Locking a room may fail: CrackTunes lacks";
pub const ROOM_AT_CATEGORY: &str = "on";
pub const ROOM_AT_HUB: &str = "on the hub";
pub const ROOM_AT_SERVER: &str = "server-wide";

// The dashboard's rooms settings page.
pub const DASH_SETTINGS_LINK: &str = "Settings";
pub const DASH_SETTINGS_TITLE: &str = "Voice rooms";
pub const DASH_SETTINGS_NEEDS_MANAGE: &str =
    "Only members with Manage Server can see or change this server's settings.";
pub const DASH_SETTINGS_ENABLED: &str = "Rooms are on";
pub const DASH_SETTINGS_HUB: &str = "Hub channel";
pub const DASH_SETTINGS_NO_HUB_OPTION: &str = "None";
pub const DASH_SETTINGS_TEMPLATE: &str = "Room name";
pub const DASH_SETTINGS_PREVIEW: &str = "Looks like:";
pub const DASH_SETTINGS_LIMIT: &str = "Default member limit (0 for none)";
pub const DASH_SETTINGS_GRACE: &str = "Delete an empty room after";
pub const DASH_SETTINGS_SAVE: &str = "Save";
pub const DASH_SETTINGS_SAVED: &str = "Saved.";
pub const DASH_SETTINGS_SAVING_FAILED: &str = "Could not reach the dashboard. Try again.";
pub const DASH_SETTINGS_GUIDE: &str = "How rooms work";
pub const DASH_SETTINGS_PERMISSIONS: &str = "Permissions";
pub const DASH_SETTINGS_NO_DATABASE: &str =
    "Rooms need the bot's database, which isn't set up, so they can't be turned on.";
pub const DASH_SETTINGS_NO_HUB: &str = "Pick a hub channel before turning rooms on.";
pub const DASH_SETTINGS_BAD_TEMPLATE: &str =
    "The room name must be 1 to 100 characters, with no control characters.";
pub const DASH_SETTINGS_BAD_LIMIT: &str = "The member limit must be from 0 to 99.";
pub const DASH_SETTINGS_BAD_GRACE: &str = "Pick one of the listed delays.";
pub const DASH_SETTINGS_UNKNOWN_CHANNEL: &str = "That hub is not a voice channel of this server.";
pub const DASH_SETTINGS_UNREADABLE: &str = "The settings could not be read.";
pub const DASH_SETTINGS_FAILED: &str = "The settings could not be saved. Try again in a moment.";
/// A settings save past the per-user rate limit: the dashboard's controls
/// say the same.
pub const DASH_SETTINGS_TOO_MANY: &str = "Slow down a little.";
pub const ROOM_SETTINGS_INVALID: &str = "Those settings aren't valid, so nothing was changed.";
pub const ROOM_SETTINGS_SAVE_FAILED: &str = "Couldn't save the settings. Try again in a moment.";
pub const ROOM_SETTINGS_NOT_LOADED: &str =
    "Room settings couldn't be read from the database yet — try again in a minute.";
pub const DASH_SETTINGS_NOT_LOADED: &str = ROOM_SETTINGS_NOT_LOADED;
pub const ROOM_SWITCHED_ON: &str = "Voice rooms are on.";
pub const ROOM_SWITCHED_OFF: &str = "Voice rooms are off. Existing rooms stay until they empty.";

// Mod log embeds.
pub const MODLOG_TITLE_MUTE: &str = "🔇 Server muted";
pub const MODLOG_TITLE_UNMUTE: &str = "🔈 Server unmuted";
pub const MODLOG_TITLE_DEAFEN: &str = "🙉 Server deafened";
pub const MODLOG_TITLE_UNDEAFEN: &str = "👂 Server undeafened";
pub const MODLOG_TITLE_DISCONNECT: &str = "⏏️ Disconnected";
pub const MODLOG_TITLE_MOVE: &str = "↪️ Moved";
pub const MODLOG_TITLE_BATCH: &str = "🗂️ Disconnected several";
pub const MODLOG_TITLE_ROOM_KICK: &str = "👢 Kicked from a room";
pub const MODLOG_TITLE_ROOM_UNBLOCK: &str = "✅ Let back into a room";
pub const MODLOG_TITLE_ROOM_MUTE: &str = "🤐 Muted in a room";
pub const MODLOG_TITLE_ROOM_UNMUTE: &str = "🗣️ Unmuted in a room";
pub const MODLOG_TITLE_IMMUNITY_GRANTED: &str = "🛡️ Immunity granted";
pub const MODLOG_TITLE_IMMUNITY_LIFTED: &str = "🛡️ Immunity lifted";
pub const MODLOG_TITLE_CREDITS_ADJUSTED: &str = "🪙 Credits adjusted";
pub const MODLOG_GRANTED_IMMUNITY_TO: &str = "granted immunity to";
pub const MODLOG_LIFTED_IMMUNITY_OF: &str = "lifted the immunity of";
pub const MODLOG_ADJUSTED_CREDITS_OF: &str = "adjusted the credits of";
pub const MODLOG_TITLE_SHOP_ACTION: &str = "🛍️ Shop action";
pub const MODLOG_TITLE_SHOP_CANCELLED: &str = "🧼 Shop pranks ended";
pub const MODLOG_SHOP_USED: &str = "used";
pub const MODLOG_SHOP_ON: &str = "on";
pub const MODLOG_SHOP_BOUNCED_PREFIX: &str = "(bounced by";
pub const MODLOG_SHOP_BOUNCED_SUFFIX: &str = "'s mirror)";
pub const MODLOG_SHOP_SIGNED: &str = "signed";
pub const MODLOG_SHOP_ANONYMOUS: &str = "anonymous";
pub const MODLOG_SHOP_CLEANSED: &str = "ended the pranks on";
pub const MODLOG_UNTIL: &str = "until";
pub const MODLOG_UNTIL_LIFTED: &str = "until lifted";
/// U+2212 MINUS SIGN, for a negative amount (the repo has no other convention).
pub const MODLOG_MINUS: &str = "\u{2212}";
pub const MODLOG_AUDIT_FOOTER: &str = "Matched from Discord's audit log by timing";
pub const MODLOG_REASON_FIELD: &str = "Reason";
pub const MODLOG_BY: &str = "by";
pub const MODLOG_BY_ONE_OF: &str = "by one of";
/// After a bot account's mention: its action is logged like a moderator's,
/// marked so readers can tell automation from a person.
pub const MODLOG_BOT_MARK: &str = "(bot)";
pub const MODLOG_FROM: &str = "from";
pub const MODLOG_TO: &str = "to";
pub const MODLOG_IN: &str = "in";
pub const MODLOG_DISCONNECTED: &str = "disconnected";
pub const MODLOG_ONE_OR_MORE_OF: &str = "One or more of";
pub const MODLOG_OF: &str = "of";
pub const MODLOG_AND: &str = "and";
pub const MODLOG_MORE: &str = "more";
pub const MODLOG_MUTED: &str = "muted";
pub const MODLOG_UNMUTED: &str = "unmuted";
pub const MODLOG_DEAFENED: &str = "deafened";
pub const MODLOG_UNDEAFENED: &str = "undeafened";
pub const MODLOG_KICKED: &str = "kicked";
pub const MODLOG_LET_BACK_IN: &str = "let back in";
pub const MODLOG_MOVED: &str = "moved";
pub const MODLOG_STATUS_ON: &str = "Mod log: **on** in";
pub const MODLOG_STATUS_OFF: &str = "Mod log: **off**";
pub const MODLOG_STATUS_READY: &str = "CrackTunes has everything it needs.";
pub const MODLOG_STATUS_UNKNOWN: &str = "Couldn't check permissions right now.";
pub const MODLOG_STATUS_VISIBLE_PREFIX: &str = "Anyone who can see";
pub const MODLOG_STATUS_VISIBLE_SUFFIX: &str = "can read the log.";
pub const MODLOG_STATUS_DASHBOARD: &str = "Dashboard settings:";
pub const MODLOG_FINDING_SERVER_GIVE: &str = "Give CrackTunes";
pub const MODLOG_FINDING_SERVER_WHERE: &str = "in Server Settings → Roles";
pub const MODLOG_FINDING_CHANNEL_IN: &str = "in";
pub const MODLOG_FINDING_SUSPENDED: &str = "Fetching is paused: CrackTunes couldn't read the audit log (it will try again in a few minutes, or when settings are saved, at most once a minute)";
pub const MODLOG_NO_DATABASE: &str = "The mod log needs a database.";
pub const MODLOG_NO_CHANNEL: &str = "Pick a channel for the log.";
pub const MODLOG_SETTINGS_NOT_LOADED: &str =
    "Mod log settings couldn't be read from the database yet — try again in a minute.";
pub const MODLOG_SETTINGS_SAVE_FAILED: &str = "Couldn't save the mod log settings — try again.";
pub const DASH_MODLOG_TITLE: &str = "Mod log";
pub const DASH_MODLOG_ENABLED: &str = "Mod log is on";
pub const DASH_MODLOG_CHANNEL: &str = "Log channel";
pub const DASH_MODLOG_NO_CHANNEL_OPTION: &str = "None";
pub const DASH_MODLOG_NO_CHANNEL_YET: &str = "No channel set yet — pick one and save.";
pub const DASH_MODLOG_GUIDE: &str = "How the mod log works";
pub const DASH_MODLOG_UNKNOWN_CHANNEL: &str = "That is not a text channel of this server.";
pub const ECONOMY_OFF: &str = "The economy is off in this server.";
pub const ECONOMY_NOT_FOR_SALE: &str = "That isn't for sale here.";
pub const ECONOMY_BUYER_OPTED_OUT: &str =
    "You've opted out of the economy, so you can't buy anything. Opt back in first.";
pub const ECONOMY_CANT_AFFORD_PREFIX: &str = "You need";
pub const ECONOMY_CANT_AFFORD_MIDDLE: &str = "more";
pub const ECONOMY_NO_DATABASE: &str = "The economy needs a database.";
pub const ECONOMY_PURCHASE_FAILED: &str =
    "That didn't work, so nothing was bought. If you were charged, it will be refunded.";
pub const ECONOMY_HEADS: &str = "Heads";
pub const ECONOMY_TAILS: &str = "Tails";
pub const ECONOMY_COIN_LANDED: &str = "The coin landed";
pub const ECONOMY_COIN_YOU_CALLED: &str = "You called";
pub const ECONOMY_COIN_WON: &str = "You win";
pub const ECONOMY_COIN_LOST: &str = "You lose";
pub const ECONOMY_COIN_BALANCE: &str = "Balance:";
pub const ECONOMY_GAMBLING_OFF: &str = "Gambling is off in this server.";
pub const ECONOMY_GAMBLER_OPTED_OUT: &str =
    "You've opted out of the economy, so you can't gamble. Opt back in first.";
pub const ECONOMY_BET_NON_POSITIVE: &str = "Bet at least 1.";
pub const ECONOMY_BET_OVER_MAX_PREFIX: &str = "The most you can bet here is";
pub const ECONOMY_YOU_HAVE: &str = "You have";
pub const ECONOMY_BET_FAILED: &str = "That didn't work, and nothing was taken. Try again.";
pub const ECONOMY_GIFT_SELF: &str = "You can't gift yourself.";
pub const ECONOMY_GIFT_TO_BOT: &str = "Bots don't need credits.";
pub const ECONOMY_GIFT_FROM_OPTED_OUT: &str =
    "You've opted out of the economy, so you can't gift. Opt back in first.";
pub const ECONOMY_GIFT_TO_OPTED_OUT: &str =
    "That member has opted out of the economy, so they can't be gifted credits.";
pub const ECONOMY_GIFT_NON_POSITIVE: &str = "Gift at least 1.";
pub const ECONOMY_GIFT_SENT: &str = "gifted";
pub const ECONOMY_GIFT_TO: &str = "to";
pub const ECONOMY_GIFT_FAILED: &str = "That didn't work, and nothing was sent. Try again.";
pub const ECONOMY_BALANCE_HAS: &str = "has";
pub const ECONOMY_TOP_TITLE: &str = "Richest members";
pub const ECONOMY_TOP_EMPTY: &str = "Nobody has any yet.";
pub const ECONOMY_HISTORY_TITLE: &str = "Your recent activity";
pub const ECONOMY_HISTORY_EARNED: &str = "Earned so far:";
pub const ECONOMY_HISTORY_IN_VOICE: &str = "in voice";
pub const ECONOMY_HISTORY_FROM_MESSAGES: &str = "from messages";
pub const ECONOMY_HISTORY_NO_ROWS: &str = "No purchases, gifts or bets yet.";
pub const ECONOMY_LEDGER_PURCHASE: &str = "purchase";
pub const ECONOMY_LEDGER_REFUND: &str = "refund";
pub const ECONOMY_LEDGER_GIFT_SENT: &str = "gift sent";
pub const ECONOMY_LEDGER_GIFT_RECEIVED: &str = "gift received";
pub const ECONOMY_LEDGER_COINFLIP_BET: &str = "coinflip bet";
pub const ECONOMY_LEDGER_COINFLIP_WIN: &str = "coinflip win";
pub const ECONOMY_LEDGER_ADJUSTMENT: &str = "adjustment";
pub const ECONOMY_LEDGER_BOUNTY_PLACED: &str = "bounty placed";
pub const ECONOMY_LEDGER_BOUNTY_WON: &str = "bounty won";
pub const ECONOMY_LEDGER_PLEDGE: &str = "pledge";
pub const ECONOMY_LEDGER_PLEDGE_REFUND: &str = "pledge refund";
pub const ECONOMY_OPTOUT_SWITCH_BACK_PREFIX: &str = "You can switch back";
pub const ECONOMY_SHOP_IMMUNITY_NOTE: &str = "Protects you from pranks.";
pub const ECONOMY_LABEL_IMMUNITY: &str = "Immunity";
pub const ECONOMY_LABEL_RENAME: &str = "Rename";
pub const ECONOMY_LABEL_LOCK: &str = "Lock nickname";
pub const ECONOMY_LABEL_CLOWN: &str = "Clown";
pub const ECONOMY_LABEL_DRAG: &str = "Drag";
pub const ECONOMY_LABEL_MUTE: &str = "Mute";
pub const ECONOMY_LABEL_DEAFEN: &str = "Deafen";
pub const ECONOMY_LABEL_JAIL: &str = "Jail";
pub const ECONOMY_LABEL_MIRROR: &str = "Mirror";
pub const ECONOMY_LABEL_CLEANSE: &str = "Cleanse";
pub const ECONOMY_LABEL_REVEAL: &str = "Reveal";
pub const ECONOMY_LABEL_THEME: &str = "Walk-on theme";
pub const ECONOMY_LABEL_DEDICATE: &str = "Dedication";
pub const ECONOMY_LABEL_JUMP: &str = "Queue jump";
pub const ECONOMY_LABEL_BOUNTY: &str = "Bounty";
pub const ECONOMY_LABEL_CROWDFUND: &str = "Crowdfund";
pub const ECONOMY_HINT_BOUNTY: &str = "Minimum stake.";
pub const ECONOMY_HINT_CROWDFUND: &str = "Minimum pledge. A fund's goal is its prank's price.";
pub const ECONOMY_HINT_JUMP: &str = "To make jumping the queue a paid perk, restrict /playnext for members in Server Settings → Integrations.";
pub const ECONOMY_SHOP_MUSIC: &str = "Music";
pub const ECONOMY_SHOP_GROUP_PLAY: &str = "Group play";
pub const ECONOMY_SHOP_FROM: &str = "from";
pub const ECONOMY_NOTE_EMPTY: &str = "The note can't be empty.";
pub const ECONOMY_NOTE_TOO_LONG: &str = "The note can be at most 200 characters.";
pub const ECONOMY_NOTE_MENTION: &str = "The note can't mention anyone.";
pub const ECONOMY_NOTE_LINK: &str = "The note can't contain a link.";
pub const ECONOMY_NICK_EMPTY: &str = "The nickname can't be empty.";
pub const ECONOMY_NICK_TOO_LONG: &str =
    "That name is too long: 32 characters at most (an emoji counts as 2 or more).";
pub const ECONOMY_NICK_MENTION: &str = "The nickname can't mention anyone.";
pub const ECONOMY_NICK_LINK: &str = "The nickname can't contain a link.";
pub const ECONOMY_CLOWN_SUFFIX: &str = " 🤡";
pub const ECONOMY_ALREADY_IMMUNE: &str = "You're already immune until a moderator lifts it.";
pub const ECONOMY_TARGET_IS_BOT: &str = "You can't prank a bot.";
pub const ECONOMY_TARGET_IS_SELF: &str = "You can't prank yourself.";
pub const ECONOMY_OUT_OF_REACH: &str =
    "CrackTunes can't change them (they're the owner, or their role is at or above CrackTunes').";
pub const ECONOMY_CANT_TELL: &str =
    "CrackTunes can't see that member right now. Try again in a moment.";
pub const ECONOMY_TARGET_LEFT: &str = "They're not in this server any more.";
pub const ECONOMY_TARGET_NOT_IN_VOICE: &str = "They need to be in a voice channel for that.";
pub const ECONOMY_BUYER_NOT_IN_VOICE: &str = "Join a voice channel first.";
pub const ECONOMY_TARGET_OPTED_OUT: &str = "They've opted out of the economy.";
pub const ECONOMY_TARGET_IMMUNE_UNTIL: &str = "They're immune until";
pub const ECONOMY_TARGET_IMMUNE_INDEFINITELY: &str = "They're immune until a moderator lifts it.";
pub const ECONOMY_ALREADY_RUNNING_PREFIX: &str = "They're already pranked like that until";
pub const ECONOMY_CANNOT_JOIN_DESTINATION: &str =
    "They couldn't join your channel themselves, so CrackTunes won't drag them there.";
pub const ECONOMY_MIRROR_ALREADY_UP: &str = "Your mirror is already up.";
pub const ECONOMY_ALREADY_MUTED: &str = "They're already server-muted.";
pub const ECONOMY_ALREADY_THERE: &str = "They're already in your channel.";
pub const ECONOMY_ALREADY_DEAFENED: &str = "They're already server-deafened.";
pub const ECONOMY_CARD_POSSESSIVE: &str = "'s";
pub const ECONOMY_CARD_SOMEONE: &str = "Someone";
pub const ECONOMY_CARD_RENAMED: &str = "renamed";
pub const ECONOMY_CARD_TO: &str = "to";
pub const ECONOMY_CARD_LOCKED: &str = "locked the nickname of";
pub const ECONOMY_CARD_CLOWNED: &str = "made a clown of";
pub const ECONOMY_CARD_DRAGGED: &str = "dragged";
pub const ECONOMY_CARD_DRAGGED_TAIL: &str = "into their voice channel";
pub const ECONOMY_CARD_MUTED: &str = "muted";
pub const ECONOMY_CARD_DEAFENED: &str = "deafened";
pub const ECONOMY_CARD_JAILED: &str = "jailed";
pub const ECONOMY_CARD_UNTIL: &str = "until";
pub const ECONOMY_BOUNTY_TOO_SMALL_PREFIX: &str = "A bounty starts at";
pub const ECONOMY_BOUNTY_PLACED: &str = "Bounty placed.";
pub const ECONOMY_BAD_LINK: &str =
    "That link can't be played. Use an http(s) link to a song or video.";
pub const ECONOMY_THEME_START_MALFORMED: &str = "Start time looks like 83, 1:23 or 1:02:03.";
pub const ECONOMY_THEME_START_TOO_LATE: &str = "Start time can be at most 12:00:00.";
pub const ECONOMY_THEME_BOUGHT_PREFIX: &str =
    "Their theme plays the next time they join voice, within 7 days. If it never does, you get your";
pub const ECONOMY_THEME_BOUGHT_SUFFIX: &str = "back.";
pub const ECONOMY_THEME_CARD_PREFIX: &str = "🎺";
pub const ECONOMY_THEME_CARD_SUFFIX: &str = "'s walk-on theme";
pub const ECONOMY_BOUNTY_CARD_PREFIX: &str = "💰";
pub const ECONOMY_BOUNTY_CARD_ON: &str = "on";
pub const ECONOMY_BOUNTY_CARD_HEAD: &str = "'s head until";
pub const ECONOMY_BOUNTY_CARD_TAIL: &str = "Land a prank on them to collect it.";
pub const ECONOMY_DEDICATE_CARD_PREFIX: &str = "🎵 This one goes out to";
pub const ECONOMY_JUMP_CARD: &str = "⏭️ Someone paid to play this next";
pub const ECONOMY_PLAY_NAME_A_SONG: &str = "Name a song or paste a link.";
pub const ECONOMY_PLAY_NOT_CHARGED: &str = "Nothing was queued, so nothing was charged.";
pub const ECONOMY_PLAY_ONE_SONG: &str = "Paid plays take one song, not a playlist.";
pub const ECONOMY_BOUNTY_SIGN_DASH: &str = "—";
pub const ECONOMY_BOUNTY_SIGN_ANON: &str = "someone";
pub const ECONOMY_BOUNTY_COLLECTED_PREFIX: &str = "💰 You collected";
pub const ECONOMY_BOUNTY_COLLECTED_SUFFIX: &str = "of bounty.";
pub const ECONOMY_BOUNTY_CARD_COLLECTED_PREFIX: &str = "💰 The bounty on";
pub const ECONOMY_BOUNTY_CARD_COLLECTED_SUFFIX: &str = "was collected.";
pub const ECONOMY_BOUNTY_LIST_TITLE: &str = "Bounties";
pub const ECONOMY_BOUNTY_LIST_NONE: &str = "No bounties right now.";
pub const ECONOMY_BOUNTY_LIST_ENDS: &str = "ends";
pub const ECONOMY_BOUNTY_LIST_FROM: &str = "from";
pub const ECONOMY_BOUNTY_LIST_MORE_PREFIX: &str = "…and";
pub const ECONOMY_BOUNTY_LIST_MORE_SUFFIX: &str = "more";
pub const ECONOMY_FUND_ALREADY_OPEN: &str =
    "A crowdfund like that is already open. Pledge to it with /crowdfund pledge.";
pub const ECONOMY_FUND_CLOSED: &str = "That crowdfund has closed.";
pub const ECONOMY_FUND_TOO_CHEAP: &str = "That prank costs too little to crowdfund.";
pub const ECONOMY_PLEDGE_ON_SELF: &str = "You can't pledge to a crowdfund against yourself.";
pub const ECONOMY_PLEDGE_TOO_SMALL_PREFIX: &str = "A pledge starts at";
pub const ECONOMY_PLEDGE_TOO_SMALL_SUFFIX: &str = "unless it covers what the fund still needs.";
pub const ECONOMY_PLEDGE_OVER_SHARE_PREFIX: &str = "You can put at most";
pub const ECONOMY_PLEDGE_OVER_SHARE_SUFFIX: &str =
    "into one crowdfund. It takes at least two people.";
pub const ECONOMY_FUND_STARTED: &str = "Crowdfund started.";
pub const ECONOMY_FUND_START_CARD_STARTED: &str = "started a crowdfund to";
pub const ECONOMY_FUND_TO_RENAME: &str = "rename";
pub const ECONOMY_FUND_TO_CLOWN: &str = "make a clown of";
pub const ECONOMY_FUND_TO_LOCK: &str = "lock the nickname of";
pub const ECONOMY_FUND_TO_MUTE: &str = "mute";
pub const ECONOMY_FUND_TO_DEAFEN: &str = "deafen";
pub const ECONOMY_FUND_TO_JAIL: &str = "jail";
pub const ECONOMY_FUND_START_CARD_TAIL: &str = "Chip in with /crowdfund pledge.";
pub const ECONOMY_FUND_PLEDGED: &str = "Pledged";
pub const ECONOMY_FUND_OF: &str = "of";
pub const ECONOMY_FUND_FILLED_LANDED: &str = "The fund is full, and it landed.";
pub const ECONOMY_FUND_FILLED_FAILED: &str = "The fund is full, but it couldn't land";
pub const ECONOMY_FUND_OPEN_HINT: &str = "Others can join with /crowdfund pledge.";
pub const ECONOMY_FUND_CARD_PREFIX: &str = "🎉 A crowdfund";
pub const ECONOMY_FUND_CARD_TO: &str = "to";
pub const ECONOMY_FUND_CARD_UNTIL: &str = "until";
pub const ECONOMY_FUND_CARD_CROWD: &str = "a crowd";
pub const ECONOMY_FUND_CARD_AND: &str = "and";
pub const ECONOMY_FUND_CARD_OTHER: &str = "other";
pub const ECONOMY_FUND_CARD_OTHERS: &str = "others";
pub const ECONOMY_FUND_FAILED_CARD_PREFIX: &str = "The crowdfund against";
pub const ECONOMY_FUND_FAILED_CARD_MIDDLE: &str = "ended";
/// The public card's reason, whatever the real one: the specific refusal
/// (an immunity, an opt-out) goes only in the pledger's private reply.
pub const ECONOMY_FUND_FAILED_CARD_WHY: &str = "it couldn't land on them right now";
pub const ECONOMY_FUND_FAILED_CARD_SUFFIX: &str = "Everyone was refunded.";
pub const ECONOMY_FUND_WHY_PERMISSIONS: &str = "CrackTunes lacks the permission";
pub const ECONOMY_FUND_WHY_NOT_IN_VOICE: &str = "they aren't in voice";
pub const ECONOMY_FUND_WHY_LEFT: &str = "they left the server";
pub const ECONOMY_FUND_WHY_OTHER: &str = "Discord refused";
pub const ECONOMY_FUND_WHY_DB: &str = "something went wrong";
pub const ECONOMY_FUND_LIST_TITLE: &str = "Open crowdfunds";
pub const ECONOMY_FUND_LIST_NONE: &str = "No crowdfunds are open.";
pub const ECONOMY_FUND_LIST_ON: &str = "on";
pub const ECONOMY_FUND_LIST_ENDS: &str = "ends";
pub const ECONOMY_FUND_LIST_MORE_PREFIX: &str = "…and";
pub const ECONOMY_FUND_LIST_MORE_SUFFIX: &str = "more";
pub const MODLOG_TITLE_CROWDFUND: &str = "🎉 Crowdfund executed";
pub const MODLOG_TITLE_CROWDFUND_STARTED: &str = "🎉 Crowdfund started";
pub const MODLOG_CROWDFUND_STARTED: &str = "started a crowdfund for";
pub const MODLOG_CROWDFUNDED_BY: &str = "crowdfunded by";
pub const ECONOMY_CARD_MIRROR: &str = "🪞";
pub const ECONOMY_CARD_MIRROR_BOUNCED: &str = "mirror bounced";
pub const ECONOMY_CARD_BACK_AT_THEM: &str = "back at them";
pub const ECONOMY_CARD_BLOCKED_TAIL: &str = "but it couldn't land on them. The credits are spent.";
pub const ECONOMY_NO_JAIL_CHANNEL: &str = "This server hasn't set a jail channel.";
pub const ECONOMY_UNAVAILABLE_PREFIX: &str = "CrackTunes is missing";
pub const ECONOMY_UNAVAILABLE_SUFFIX: &str = "for that.";
pub const ECONOMY_ADMIN_ADJUST_TOO_BIG: &str = "Adjust by at most 1,000,000,000 at a time.";
pub const ECONOMY_READ_FAILED: &str = "Couldn't read that right now. Try again.";
pub const ECONOMY_OPTED_OUT_DONE: &str =
    "You've opted out. You won't earn, buy, gamble, gift or be gifted, and your balance is kept.";
pub const ECONOMY_OPTED_IN_DONE: &str = "You've opted back in.";
pub const ECONOMY_OPTOUT_DISABLED: &str = "This server doesn't allow opting out.";
pub const ECONOMY_OPTOUT_ALREADY_OUT: &str = "You've already opted out.";
pub const ECONOMY_OPTOUT_ALREADY_IN: &str = "You're already opted in.";
pub const ECONOMY_OPTOUT_COOLDOWN_PREFIX: &str = "You switched recently. You can switch again";
pub const ECONOMY_OPTOUT_FAILED: &str = "Couldn't change that. Try again.";
pub const ECONOMY_SHOP_TITLE: &str = "Shop";
pub const ECONOMY_SHOP_NOT_SOLD: &str = "not sold here";
pub const ECONOMY_SHOP_DOCS_LABEL: &str = "How the economy works";
pub const ECONOMY_DOCS_URL: &str =
    "https://github.com/cycle-five/cracktunes/blob/master/docs/economy.md";
pub const ECONOMY_IMMUNE_UNTIL: &str = "is immune until";
pub const ECONOMY_SHOP_BOT_BUYER: &str = "Bots can't buy anything.";
pub const ECONOMY_NOTHING_TO_CLEANSE: &str = "Nothing's running on you.";
pub const ECONOMY_NOTHING_TO_REVEAL: &str = "There's nothing to unmask.";
pub const ECONOMY_ALREADY_REVEALED: &str = "You've already unmasked that one.";
pub const ECONOMY_SHOP_DONE: &str = "Done.";
pub const ECONOMY_SHOP_MIRROR_UP: &str = "Your mirror is up. Nobody can see it.";
pub const ECONOMY_SHOP_BOUNCED: &str = "Their mirror sent it back at you.";
pub const ECONOMY_SHOP_BLOCKED: &str = "Their mirror blocked it, and you were still charged.";
pub const ECONOMY_SHOP_CLEANSED_PREFIX: &str = "Cleansed. Ended";
pub const ECONOMY_SHOP_CLEANSED_SUFFIX: &str = "running on you.";
pub const ECONOMY_REVEAL_PREFIX: &str = "🔍";
pub const ECONOMY_REVEAL_UNMASKED: &str = "unmasked the";
pub const ECONOMY_REVEAL_WAS: &str = "It was";
pub const ECONOMY_REVEAL_PICK_JUST_NOW: &str = "just now";
pub const ECONOMY_REVEAL_PICK_AGO: &str = "ago";
pub const ECONOMY_REVEAL_PICK_MINUTE: &str = "minute";
pub const ECONOMY_REVEAL_PICK_HOUR: &str = "hour";
pub const ECONOMY_REVEAL_PICK_DAY: &str = "day";
pub const ECONOMY_CANCEL_NONE: &str = "Nothing's running on them.";
pub const ECONOMY_CANCEL_DONE_PREFIX: &str = "Ended";
pub const ECONOMY_CANCEL_DONE_SUFFIX: &str = "running on them.";
pub const ECONOMY_STATUS_TITLE: &str = "Economy status";
pub const ECONOMY_STATUS_ON: &str = "on";
pub const ECONOMY_STATUS_OFF: &str = "off";
pub const ECONOMY_STATUS_CURRENCY: &str = "Currency";
pub const ECONOMY_STATUS_WALLETS: &str = "Wallets";
pub const ECONOMY_STATUS_HELD: &str = "Held in total";
pub const ECONOMY_STATUS_EARNING: &str = "Earning";
pub const ECONOMY_STATUS_PER_VOICE_MINUTE: &str = "per voice minute";
pub const ECONOMY_STATUS_PER_MESSAGE: &str = "per message, at most once every";
pub const ECONOMY_STATUS_SECONDS: &str = "s";
pub const ECONOMY_STATUS_IMMUNITY: &str = "Immunity: opt-out";
pub const ECONOMY_STATUS_SHOP: &str = "shop";
pub const ECONOMY_STATUS_MODS: &str = "mod grants";
pub const ECONOMY_STATUS_GAMBLING: &str = "Gambling";
pub const ECONOMY_STATUS_MAX_BET: &str = "max bet";
pub const ECONOMY_STATUS_RECENT: &str = "Recent shop actions";
pub const ECONOMY_STATUS_RECENT_NONE: &str = "Nothing bought yet.";
pub const ECONOMY_STATUS_RECENT_MORE_PREFIX: &str = "…and";
pub const ECONOMY_STATUS_RECENT_MORE_SUFFIX: &str = "older not shown.";
pub const ECONOMY_STATUS_UNTIL: &str = "until";
pub const ECONOMY_ADMIN_BOT_TARGET: &str = "Bots don't take part in the economy.";
pub const ECONOMY_ADMIN_HOURS_RANGE: &str = "Hours must be between 1 and 8760 (a year).";
pub const ECONOMY_ADMIN_GRANT_DISABLED: &str =
    "This server has turned off moderator immunity grants.";
pub const ECONOMY_ADMIN_GRANT_DONE: &str = "Immunity granted.";
pub const ECONOMY_ADMIN_GRANT_FAILED: &str = "Couldn't grant that right now. Try again.";
pub const ECONOMY_ADMIN_LIFT_DONE: &str = "Moderator immunity lifted.";
pub const ECONOMY_ADMIN_LIFT_NONE: &str = "That member has no moderator grant to lift.";
pub const ECONOMY_ADMIN_LIFT_FAILED: &str = "Couldn't lift that right now. Try again.";
pub const ECONOMY_ADMIN_ADJUST_NEEDS_MANAGE: &str =
    "Adjusting credits needs the Manage Server permission.";
pub const ECONOMY_ADMIN_ADJUST_ZERO: &str = "Adjust by a nonzero amount.";
pub const ECONOMY_ADMIN_ADJUST_BELOW_ZERO_PREFIX: &str =
    "That would take them below zero; they hold";
pub const ECONOMY_ADMIN_ADJUST_DONE: &str = "Adjusted. They now hold";
pub const ECONOMY_ADMIN_ADJUST_FAILED: &str = "Couldn't adjust that right now. Try again.";
pub const ECONOMY_ADMIN_NEEDS_MODERATE: &str =
    "This command needs the Moderate Members permission.";

// ---- Economy: the dashboard section ----
pub const DASH_ECONOMY_TITLE: &str = "Economy";
pub const DASH_ECONOMY_ENABLED: &str = "Economy is on";
pub const DASH_ECONOMY_CURRENCY_NAME: &str = "Currency name";
pub const DASH_ECONOMY_CURRENCY_EMOJI: &str = "Currency emoji";
pub const DASH_ECONOMY_VOICE_RATE: &str = "Credits per minute in voice";
pub const DASH_ECONOMY_MESSAGE_RATE: &str = "Credits per message";
pub const DASH_ECONOMY_MESSAGE_COOLDOWN: &str = "Seconds between paid messages";
pub const DASH_ECONOMY_PRICES: &str = "Shop prices";
pub const DASH_ECONOMY_PRICE_FOR_SALE: &str = "For sale";
pub const DASH_ECONOMY_PRICE_COST: &str = "Price";
pub const DASH_ECONOMY_IMMUNITY_OPTOUT: &str = "Members may opt out of the economy for immunity";
pub const DASH_ECONOMY_IMMUNITY_SHOP: &str = "Members may buy immunity in the shop";
pub const DASH_ECONOMY_IMMUNITY_MODS: &str = "Moderators may grant immunity";
pub const DASH_ECONOMY_OPTOUT_COOLDOWN: &str = "Days between opting out and back in";
pub const DASH_ECONOMY_IMMUNITY_HOURS: &str = "Hours of bought immunity";
pub const DASH_ECONOMY_GAMBLING: &str = "Gambling (coinflip) is on";
pub const DASH_ECONOMY_MAX_BET: &str = "Largest bet";
pub const DASH_ECONOMY_GAMBLING_NOTE: &str =
    "Credits have no cash value. Only turn this on where your members are adults.";
pub const DASH_ECONOMY_GUIDE: &str = "How the economy works";
pub const DASH_ECONOMY_NO_DATABASE: &str = "The economy needs a database.";
pub const DASH_ECONOMY_BAD_CURRENCY_NAME: &str =
    "The currency name must be 1 to 24 characters, with no control characters.";
pub const DASH_ECONOMY_BAD_CURRENCY_EMOJI: &str =
    "The currency emoji must be 1 to 64 characters, with no spaces or control characters.";
pub const DASH_ECONOMY_BAD_RATE: &str = "Earning rates must be between 0 and 100.";
pub const DASH_ECONOMY_BAD_COOLDOWN: &str =
    "The message cooldown must be between 10 and 3600 seconds.";
pub const DASH_ECONOMY_BAD_PRICE: &str = "Every price must be between 1 and 1,000,000.";
pub const DASH_ECONOMY_BAD_OPTOUT_COOLDOWN: &str =
    "The opt-out cooldown must be between 0 and 90 days.";
pub const DASH_ECONOMY_BAD_IMMUNITY_HOURS: &str =
    "Bought immunity must last between 1 and 720 hours.";
pub const DASH_ECONOMY_BAD_MAX_BET: &str = "The largest bet must be between 1 and 1,000,000.";
pub const DASH_ECONOMY_BAD_RENAME_HOURS: &str =
    "Hours a rename, lock or clown lasts must be between 1 and 168.";
pub const DASH_ECONOMY_BAD_SHORT_EFFECT: &str =
    "Minutes a mute, deafen or jail lasts must be between 1 and 60.";
pub const DASH_ECONOMY_RENAME_HOURS: &str = "Hours a rename, lock or clown lasts";
pub const DASH_ECONOMY_SHORT_EFFECT: &str = "Minutes a mute, deafen or jail lasts";
pub const DASH_ECONOMY_JAIL_CHANNEL: &str = "Jail channel";
pub const DASH_ECONOMY_ANNOUNCE_CHANNEL: &str = "Announcement channel";
pub const DASH_ECONOMY_NO_CHANNEL_OPTION: &str = "None";
pub const DASH_ECONOMY_JAIL_NOTE: &str = "Members are moved here even if they can't see it.";
pub const DASH_ECONOMY_UNKNOWN_CHANNEL: &str =
    "The jail channel must be a voice channel and the announcement channel a text channel of this server.";
pub const DASH_ECONOMY_PREFLIGHT: &str = "What CrackTunes needs for each prank";
pub const DASH_ECONOMY_PREFLIGHT_READY: &str = "Ready.";
pub const DASH_ECONOMY_PREFLIGHT_UNKNOWN: &str = "Couldn't check permissions right now.";
pub const DASH_ECONOMY_PREFLIGHT_NO_JAIL: &str = "Choose a jail channel first.";
pub const DASH_ECONOMY_ANNOUNCE_STATUS: &str = "Announcements";
pub const DASH_ECONOMY_FINDING_GIVE: &str = "Give CrackTunes";
pub const DASH_ECONOMY_FINDING_SERVER: &str = "in Server Settings → Roles";
pub const DASH_ECONOMY_FINDING_IN: &str = "in";
pub const DASH_ECONOMY_UNKNOWN_ACTION: &str = "That is not something this server can sell.";
pub const DASH_ECONOMY_SETTINGS_NOT_LOADED: &str =
    "Economy settings couldn't be read from the database yet — try again in a minute.";
pub const DASH_ECONOMY_SAVE_FAILED: &str = "Couldn't save the economy settings — try again.";
pub const ECONOMY_BUTTON_RENAME_BACK: &str = "Rename them back";
pub const ECONOMY_BUTTON_REVEAL: &str = "Reveal";
pub const ECONOMY_BACK_REVEAL_FIRST: &str = "Reveal who did it first.";
pub const ECONOMY_BUTTON_NOT_YOURS: &str = "Only the person it was done to can use these buttons.";
pub const ECONOMY_BUTTON_STALE: &str = "That card is out of date.";
pub const ECONOMY_BACK_MODAL_TITLE: &str = "Rename them back";
pub const ECONOMY_BACK_LABEL: &str = "New nickname";
pub const ECONOMY_BACK_DESC: &str = "1 to 32 characters. This is bought like /shop rename.";
pub const ECONOMY_BACK_PLACEHOLDER: &str = "Up to 32 characters";
