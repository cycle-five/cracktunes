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
pub const FAIL_AUTHOR_DISCONNECTED: &str = "⚠️ You are not connected to";
///?
pub const FAIL_AUTHOR_NOT_FOUND: &str = "⚠️ Could not find you in any voice channel!";
pub const FAIL_LOOP: &str = "⚠️ Failed to toggle loop!";
pub const FAIL_EMPTY_VECTOR: &str = "⚠️ Empty vector not allowed!";
pub const FAIL_INSERT: &str = "⚠️ Failed to insert!";
pub const FAIL_INVALID_TOPGG_TOKEN: &str = "⚠️ Invalid top.gg token!";
pub const FAIL_INVALID_PERMS: &str = "⚠️ Invalid permissions!!";
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

pub const GUILD_ONLY: &str = "⚠️ This command can only be used in a server!";
pub const IDLE_ALERT: &str = "⚠️ I've been idle for a while so I'm going to hop off, set the idle timeout to change this! Also support my development and I won't have to premium-gate features!\n[CrackTunes Patreon](https://patreon.com/CrackTunes)";
pub const IP_DETAILS: &str = "🌐 IP details for";
pub const JOINING: &str = "Joining";
pub const KICKED: &str = "Kicked";
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
pub const PREMIUM_PLUG: &str = "👑 Like the bot? Support my development and keep it premium-free for everyone!\n[CrackTunes Patreon](https://patreon.com/CrackTunes)";
pub const PROGRESS: &str = "Progress";
pub const PHONE_NUMBER_INFO_ERROR: &str = "⚠️ Failed to fetch phone number info!";
pub const QUEUE_EXPIRED: &str = "This command has expired.\nPlease feel free to reinvoke it!";
pub const QUEUE_IS_EMPTY: &str = "Queue is empty!";
pub const QUEUE_NO_SONGS: &str = "There's no songs up next!";
pub const QUEUE_NO_TITLE: &str = "Unknown title";
pub const QUEUE_NO_SRC: &str = "Unknown source url";
pub const QUEUE_NOTHING_IS_PLAYING: &str = "Nothing is playing!";
pub const QUEUE_NOW_PLAYING: &str = "🔊 Now playing";
pub const QUEUE_PAGE_OF: &str = "of";
pub const QUEUE_PAGE: &str = "Page";
pub const QUEUE_UP_NEXT: &str = "⌛ Up next";
pub const REMOVED_QUEUE_MULTIPLE: &str = "❌ Removed multiple tracks from queue!";
pub const REMOVED_QUEUE: &str = "❌ Removed from queue";
pub const RESUMED: &str = "▶ Resumed!";
pub const REQUESTED_BY: &str = "Requested by";
pub const ROLE_CREATED: &str = "📝 Created role!";
pub const ROLE_DELETED: &str = "🗑️ Deleted role!";
pub const ROLE_NOT_FOUND: &str = "⚠️ Role not found!";
pub const SCAN_QUEUED: &str = "🔍 Scan queued! Use";
pub const SEARCHING: &str = "🔎 Searching...";
pub const SEEKED: &str = "⏩ Seeked current track to";
pub const SEEK_FAIL: &str = "❌⏩ Failed to seek to";
pub const SHUFFLED_SUCCESS: &str = "🔀 Shuffled successfully!";
pub const SKIP_VOTE_EMOJI: &str = "🗳";
pub const SKIP_VOTE_MISSING: &str = "more vote(s) needed to skip!";
pub const SKIP_VOTE_USER: &str = "has voted to skip!";
pub const SKIPPED_ALL: &str = "⏭️ Skipped until infinity!";
pub const SKIPPED_TO: &str = "⏭️ Skipped to";
pub const SKIPPED: &str = "⏭️ Skipped!";
// The old copy here asked "Did you forget to provide your Spotify application's
// client ID and secret?" -- advice that stopped being actionable. Spotify has
// blocked the creation of new Web API apps since roughly December 2025, so an
// operator without credentials often CANNOT obtain them, and sending them to the
// dashboard wastes their time. Tell the person in the channel what they can
// actually do instead; the operator gets the real diagnosis in the logs.
pub const SPOTIFY_AUTH_FAILED: &str = "⚠️ **Spotify links aren't available right now.**\nI can't look this up, but I can still play it — try searching for the track or artist by name.";
pub const AUTOPLAY_DISABLED_SPOTIFY: &str = "🤖 **Autoplay is off.**\nPicking the next track needs Spotify, which isn't available right now. Queue something up and I'll keep playing.";
pub const AUTOPLAY_DISABLED_ERROR: &str =
    "🤖 **Autoplay is off.**\nI couldn't work out what to play next.";
// Operator-facing. Never sent to a channel -- this is the detail a person
// running the bot needs, and the detail a Discord user cannot act on.
pub const SPOTIFY_DISABLED_LOG: &str = "spotify: DISABLED -- Spotify links and autoplay will not work. Note that Spotify has blocked new Web API app creation since ~2025-12, so this may not be fixable simply by supplying credentials.";
pub const SPOTIFY_ENABLED_LOG: &str = "spotify: enabled -- client credentials accepted";
pub const SPOTIFY_INVALID_QUERY: &str =
    "⚠️ **Could not find any tracks with that link!**\nAre you sure that is a valid Spotify URL?";
pub const SPOTIFY_PLAYLIST_FAILED: &str = "⚠️ **Failed to fetch playlist!**\nIt's likely that this playlist is either private or a personalized playlist generated by Spotify, like your daylist.";
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
pub const GP_RULES_TEXT: &str = "The host picks a category. Each round the bot posts a prompt and everyone in the voice channel secretly submits a song for it. The songs then play one by one: guess whose is whose, and 👍 the ones you like. The submitter is revealed when each song ends.";
pub const GP_HOW_TO_TITLE: &str = "How to play";
pub const GP_HOW_TO: &str = "• `/gp start <category> [rounds] [timer]` — start in your voice channel (default 5 rounds, 3 minutes to submit)\n• `/gp submit <song>` — your song for the current prompt; only you see the reply; submitting again replaces it\n• The window closes on the timer, as soon as everyone in the voice channel is in, or when the host runs `/gp close`\n• While a song plays: pick who submitted it from the dropdown (you can change your pick until it ends) and tap 👍 if you like it\n• `/gp skip` — host ends the current song · `/gp voteskip` — a majority ends it, or pulls it outright if it is your own · `/gp status` — where things stand · `/gp end` — abort\n• You need to be in the game's voice channel, and to have submitted a song, to guess, 👍 or vote\n\n**Scoring:** +100 for a correct guess · +100 if nobody guesses you · +10 for every 👍 your song gets. Up to 25 people per round.";
pub const GP_STARTED: &str = "🎭 Game on!";
pub const GP_STARTED_ROUNDS: &str = "rounds of";
pub const GP_STARTED_TIMER: &str = "to submit each round.";
pub const GP_QUEUE_CLEARED: &str = "(cleared the existing queue)";
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
pub const GP_VOTESKIP_PASSED: &str = "🗳️ The room voted to skip this one.";
pub const GP_VOTESKIP_OWN: &str = "🙈 Your song, your call — pulled it.";
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
pub const FAIL_GP_NOT_A_GAME_PLAYER: &str =
    "🎭 You're not in this game — `/gp submit <song>` on the next prompt to join in.";
