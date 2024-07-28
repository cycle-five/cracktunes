# Change Log

## TODO:

- [ ] /changenicks command. Renames all users in the guild
      to a random nick name from a themed list of names. Use your
      own custom list, or choose from one of the many I've
      pre-curated and use in my own server.
- [ ] Test Coverage > 23%.
- [ ] Codebase architecture documentation.
- [ ] Support discordbotlist.com (voting service).

## v0.3.10 (ETA: 2024/08/01)

- [x] performance improvements.
- [x] All milestones recorded as GitHub issues.
- [x] Add help option to all commands.
- [x] Re-enable playlist support.

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

