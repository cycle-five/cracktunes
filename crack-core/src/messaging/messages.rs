pub const ADDED_QUEUE: &str = "📃 Added to queue!";
pub const AUTOPAUSE_OFF: &str = "🤖 Autopause OFF!";
pub const AUTOPAUSE_ON: &str = "🤖 Autopause ON!";
pub const CLEARED: &str = "🗑️ Cleared!";
pub const CHANNEL_DELETED: &str = "🗑️ Deleted channel!";

pub const BANNED: &str = "Banned";
pub const UNBANNED: &str = "Unbanned";
pub const DEAFENED: &str = "Deafened";
pub const UNDEAFENED: &str = "Undeafened";
pub const MUTED: &str = "Muted";
pub const UNMUTED: &str = "Unmuted";

pub const DOMAIN_FORM_ALLOWED_TITLE: &str = "Allowed domains";
pub const DOMAIN_FORM_BANNED_TITLE: &str = "Banned domains";
pub const DOMAIN_FORM_ALLOWED_PLACEHOLDER: &str =
    "Add domains separated by \';\'. If left blank, all (except for banned) are allowed by default.";
pub const DOMAIN_FORM_BANNED_PLACEHOLDER: &str =
    "Add domains separated by \';\'. If left blank, all (except for allowed) are blocked by default.";
pub const DOMAIN_FORM_TITLE: &str = "Manage sources";

pub const ERROR: &str = "Fatality! Something went wrong ☹️";
pub const FAIL_ALREADY_HERE: &str = "⚠️ I'm already here!";
pub const FAIL_ANOTHER_CHANNEL: &str = "⚠️ I'm already connected to";
pub const FAIL_AUTHOR_DISCONNECTED: &str = "⚠️ You are not connected to";
pub const FAIL_AUTHOR_NOT_FOUND: &str = "⚠️ Could not find you in any voice channel!";
pub const FAIL_LOOP: &str = "⚠️ Failed to toggle loop!";
pub const FAIL_MINUTES_PARSING: &str = "⚠️ Invalid formatting for 'minutes'";
pub const FAIL_NO_SONG_ON_INDEX: &str = "⚠️ There is no queued song on that index!";
pub const FAIL_NO_VOICE_CONNECTION: &str = "⚠️ I'm not connected to any voice channel!";
pub const FAIL_NOTHING_PLAYING: &str = "🔈 Nothing is playing!";
pub const FAIL_REMOVE_RANGE: &str = "⚠️ `until` needs to be higher than `index`!";
pub const FAIL_SECONDS_PARSING: &str = "⚠️ Invalid formatting for 'seconds'";
pub const FAIL_WRONG_CHANNEL: &str = "⚠️ We are not in the same voice channel!";
pub const FAIL_PARSE_TIME: &str = "⚠️ Failed to parse time, speak English much?";
pub const FAIL_PLAYLIST_FETCH: &str = "⚠️ Failed to fetch playlist!";
pub const FAIL_INVALID_IP: &str = "⚠️ Invalid IP address!";
pub const IDLE_ALERT: &str = "I've been idle for a while, so I'll leave to annoy you.\nPay for my bad habits and get the premium version of the bot to stop this!";
pub const IP_DETAILS: &str = "🌐 IP details for";
pub const JOINING: &str = "Joining";
pub const LEAVING: &str = "👋 See you soon!";
pub const LOOP_DISABLED: &str = "🔁 Disabled loop!";
pub const LOOP_ENABLED: &str = "🔁 Enabled loop!";
pub const NO_GUILD_ID: &str = "⚠️ No GuildId Found!";
pub const NO_GUILD_SETTINGS: &str = "⚠️ No GuildSettings Found!";
pub const ONETWOFT: &str = "https://12ft.io/";
pub const PASSWORD_PWNED: &str = "⚠️ This password has been pwned!";
pub const PASSWORD_SAFE: &str = "🔒 This password is safe!";
pub const PAUSED: &str = "⏸️ Paused!";
pub const PLAYLIST_CREATED: &str = "📃 Created playlist!";
pub const PLAY_FAILED_BLOCKED_DOMAIN: &str =
    "**is either not allowed in this server or is not supported!** \n\nTo explicitely allow this domain, ask a moderator to run the `/managesources` command. [Click to see a list of supported sources.](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md)";
pub const PLAY_ALL_FAILED: &str =
    "⚠️ Cannot fetch playlist via keywords! Try passing this command an URL.";
pub const PLAY_PLAYLIST: &str = "📃 Added playlist to queue!";
pub const PLAY_SEARCH: &str = "🔎 Searching...";
pub const PHONE_NUMBER_INFO_ERROR: &str = "⚠️ Failed to fetch phone number info!";
pub const PLAY_QUEUE: &str = "📃 Added to queue!";
pub const PLAY_TOP: &str = "📃 Added to top!";
pub const QUEUE_EXPIRED: &str = "This command has expired.\nPlease feel free to reinvoke it!";
pub const QUEUE_IS_EMPTY: &str = "Queue is empty!";
pub const QUEUE_NO_SONGS: &str = "There's no songs up next!";
pub const QUEUE_NO_TITLE: &str = "Unknown title";
pub const QUEUE_NO_SRC: &str = "Unknown source url";
pub const QUEUE_NOTHING_IS_PLAYING: &str = "Nothing is playing!";
pub const QUEUE_NOW_PLAYING: &str = "🔊 Now playing";
pub const QUEUE_PAGE_OF: &str = "of";
pub const QUEUE_PAGE: &str = "Page";
pub const QUEUE_UP_NEXT: &str = "⌛ Up next";
pub const REMOVED_QUEUE_MULTIPLE: &str = "❌ Removed multiple tracks from queue!";
pub const REMOVED_QUEUE: &str = "❌ Removed from queue";
pub const RESUMED: &str = "▶️ Resumed!";
pub const ROLE_CREATED: &str = "📝 Created role!";
pub const ROLE_DELETED: &str = "🗑️ Deleted role!";
pub const SEARCHING: &str = "🔎 Searching...";
pub const SEEKED: &str = "⏩ Seeked current track to";
pub const SHUFFLED_SUCCESS: &str = "🔀 Shuffled successfully!";
pub const SKIP_VOTE_EMOJI: &str = "🗳 ";
pub const SKIP_VOTE_MISSING: &str = "more vote(s) needed to skip!";
pub const SKIP_VOTE_USER: &str = "has voted to skip!";
pub const SKIPPED_ALL: &str = "⏭️ Skipped until infinity!";
pub const SKIPPED_TO: &str = "⏭️ Skipped to";
pub const SKIPPED: &str = "⏭️ Skipped!";
pub const SPOTIFY_AUTH_FAILED: &str = "⚠️ **Could not authenticate with Spotify!**\nDid you forget to provide your Spotify application's client ID and secret?";
pub const SPOTIFY_INVALID_QUERY: &str =
    "⚠️ **Could not find any tracks with that link!**\nAre you sure that is a valid Spotify URL?";
pub const SPOTIFY_PLAYLIST_FAILED: &str = "⚠️ **Failed to fetch playlist!**\nIt's likely that this playlist is either private or a personalized recommendation playlist generated by Spotify.";
pub const STOPPED: &str = "⏹️ Stopped!";
pub const TRACK_DURATION: &str = "Track duration: ";
pub const TRACK_NOT_FOUND: &str = "⚠️ **Could not play track!**\nYour request yielded no results.";
pub const TRACK_INAPPROPRIATE: &str = "⚠️ **Could not play track!**\nThe video you requested may be inappropriate for some users, so sign-in is required.";
pub const TRACK_TIME_TO_PLAY: &str = "Estimated time until play: ";
pub const TEXT_CHANNEL_CREATED: &str = "📝 Created text channel!";
pub const UNAUTHORIZED_USER: &str = "⚠️ You are not authorized to use this command!";
pub const WAYBACK_SNAPSHOT: &str = "Wayback snapshot for";
pub const KICKED: &str = "Kicked ";
pub const VERSION_LATEST: &str = "Find the latest version [here]";
pub const VERSION: &str = "Version";
pub const VOLUME: &str = "🔊 Volume";
pub const VOICE_CHANNEL_CREATED: &str = "🔊 Created voice channel!";
