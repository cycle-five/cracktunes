# Investigation: CrackTunes on Matrix

2026-10-22. **Verdict: not feasible now.** We will keep the seam in mind and design new
work so a second platform stays possible.

## The question

Could CrackTunes run on Matrix as well as Discord, with Discord as just one interface to a
platform-neutral core? People the owner knows would use it on Matrix. **Music is what they
would want first**: someone in a call queues a song and everyone hears it.

## What we found: the Matrix side

Playing music into a Matrix call is possible, and open-source bots already do it.

- **Calls run through Element Call (MatrixRTC) on a LiveKit server**, the SFU. They are not
  peer-to-peer.
- **A bot plays music by joining the call as an ordinary participant:**
  1. It announces its membership in the call with a MatrixRTC event.
  2. It exchanges a Matrix OpenID token for a LiveKit token at the homeserver's call-auth
     service (`lk-jwt-service`).
  3. It connects to LiveKit and publishes an audio track.
- **Existing bots, all Python, are working references:**
  - [ECMBot](https://github.com/FOAGF/elementcall-musicbot): ffmpeg, encrypted calls via mautrix.
  - [matrixRadioBot](https://github.com/shabane/matrixRadioBot): yt-dlp and ffmpeg, our
    pipeline.
  - [jellyfin-to-matrix-music-bot](https://github.com/ricardo-duarte-av/jellyfin-to-matrix-music-bot),
    whose README spells out the join steps.

  [matrixrtc-recorder](https://github.com/ricardo-duarte-av/matrixrtc-recorder) is a reference
  for membership and keys. None of them is in Rust.
- **The Rust pieces exist.** LiveKit's official Rust SDK publishes raw PCM from a server
  (`NativeAudioSource` → `LocalAudioTrack`; see the
  [raw tracks docs](https://docs.livekit.io/home/client/tracks/raw-tracks)), and
  `matrix-rust-sdk` covers the chat side. The audio has to be fed at real-time pace, because
  `capture_frame` blocks on a 50 ms buffer.

### Why not now

- **The homeserver must host call infrastructure:** a LiveKit SFU and its auth service,
  advertised in `.well-known/matrix/client` (`org.matrix.msc4143.rtc_foci`). The bot works
  only where calls are set up that way.
- **MatrixRTC is still moving.** Membership is moving from legacy state events to sticky
  events, and the token endpoint is changing. Bots today speak both dialects.
- **Encrypted calls need per-participant key exchange on top.**
- **Most of the work is on our side, not Matrix's** (next section).

## What we found: our side

- **The queue is not ours. This is the main knot.** It lives inside songbird's Discord voice
  `Call` (`TrackQueue`), and each track's metadata hangs off songbird `TrackHandle` data.
  There are about 110 queue references across about 20 files. A Matrix player can't use
  songbird, which speaks Discord's voice protocol. The core would have to own the queue, and
  each platform would supply a player that plays one track and reports when it ends.
- **Two seams already exist, but both speak Discord types.**
  - The ops layer (`crack-core/src/music/ops`) is the one place playback and queue changes are
    orchestrated, for slash commands and the dashboard alike. Its `OpCx` carries serenity's
    `Http`, `Cache` and `GuildId`.
  - The messaging `Transport` (`crack-core/src/messaging/transport.rs`) sends and edits
    messages, but in `GenericChannelId` and `MessageId`.
- **`crack-types` (about 1,900 lines) is a grab bag tied to Discord.** It holds yt-dlp URL
  safety, Spotify types, re-exports of songbird (`AuxMetadata`, `YoutubeDl`, `SongbirdCall`)
  and serenity (`UserId`, `Attachment`), test fixtures and lock aliases. It depends on
  serenity, songbird and poise. The crate meant to be shared is itself platform-specific.
- **`crack-core` is about 116k lines, nearly the whole bot.** Of its modules, these
  reference serenity, songbird or poise:
  - music: 18 of 25 files;
  - commands: 125 of 132;
  - rooms: 15 of 16;
  - economy: 17 of 18.
- **Some features are Discord by nature and would stay there:** voice rooms, the mod log, and
  the economy's pranks (mute, deafen, rename, jail).

## Keep the seam in mind

The decision: no Matrix work now, but new work should not make it harder. Concretely:

1. **Don't deepen the songbird queue.** New queue or track state goes in CrackTunes-owned
   types (alongside `ResolvedTrack`, the play log and the audit), not in more `TrackHandle`
   data or `TrackQueue` tricks.
2. **Keep the logic pure and platform-neutral where a plain value will do.** The economy's
   rules (`earn.rs`'s `voice_earners`, `checks.rs`) and the music ops already take snapshots
   and return decisions; follow that shape. Pass plain values in, and keep serenity and
   songbird at the edges.
3. **Route Discord effects through the existing seams.** Send through the ops layer and
   `Transport`, rather than calling serenity directly from new logic.
4. **When `crack-types` is rethought,** split platform-neutral types (tracks, queries,
   metadata, URL safety) from Discord glue. The neutral half must not depend on serenity,
   songbird or poise.

## If we pick this up later

In order:
1. The core owns the queue; songbird becomes a Discord player behind a trait.
2. Platform-neutral ids and an actor or context in the ops layer.
3. A Matrix crate: `matrix-rust-sdk` for commands and replies, LiveKit for audio, with the
   MatrixRTC join copied from a working reference bot.
4. Find out first whether the users' homeserver runs Element Call with a LiveKit backend.
