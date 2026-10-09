# `/gp` on the courier

**Status:** approved in conversation, 2026-10-09. The owner approved sections 1 and 2 and said to take it through to a PR; sections 3 and 4 are reviewed in the PR.
**Arc:** the messaging layer (`2026-10-07-messaging-layer-and-now-playing-buttons-design.md`), follow-up 1: "`/gp` onto the courier: `gp.rs` and `gp_persist.rs`, with their own components and lifecycle."

## Why

The owner's intent for the arc is that every message goes through one abstraction, so the quirks and annoyances get hammered out in one place. v0.22.0 and v0.23.0 moved the music path. `/gp` was left behind on purpose, behind 22 `#[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]`:

| file | expects |
|---|---|
| `commands/music/gp/commands.rs` | 6 |
| `commands/music/gp/playback.rs` | 10 |
| `commands/music/gp_persist.rs` | 6 |

`/gp` also skips the shared formatter. `gp/ui.rs` never calls `format::escape`, `clip` or `cap`, so a song title with markdown in it (`*NSYNC`, `__init__`) renders as markdown, and no length is capped.

Today only `/gp`'s pure state is tested. Nothing tests the Discord glue: the round prompt, the abort, the close, the reveal, the results, the scoreboard, or the resume after a restart. These send through `Arc<Http>`, so no test can see them.

## Decisions

| question | ruling |
|---|---|
| Scope | **A pure move.** #469 (owed results can repeat on reconnect) and #423 (`never_played` conflates a dead link with a song cut short) are follow-ups on the new seam, not part of this PR. |
| Message model | **Approach 1:** one `CrackedMessage::Gp(Box<GpCard>)` variant, rendered by `gp::ui::render_card`. `render()` stays the only path to Discord output, and `/gp`'s look stays in `gp/`, where its contributor works. |
| Formatter | **Adopted.** Song titles are escaped and capped like the music path's since v0.22.0. This is the one visible difference, and it only shows on titles with markdown in them or ones that are very long. |
| #592 (Save button) | Christian rebases onto this. A comment on #592 says the seam is moving. |
| Button answers | `/gp` keeps its one-step ephemeral answer. It does **not** take the now-playing buttons' acknowledge-then-follow-up (see section 3). |

Out of scope: admin, osint, settings, register and utility (follow-up 2 of the arc); #469; #423; #592; and every other open `/gp` issue.

## 1. Message model

`GpCard` is an enum owned by `/gp`, in `gp/ui.rs`. It derives `Debug`, which `CrackedMessage` requires. Each variant owns the state value its builder takes today, cloned at the send site. The values are small, and `/gp` sends a handful of messages a minute.

| variant | message | renders as |
|---|---|---|
| `Rules` | `/gp` | embed (`gp_rules_embed`) |
| `Status(GpStatus)` | `/gp status` | embed (`gp_status_embed`) |
| `Picker { text: String, picked: Vec<GpCategory>, open: bool }` | the category picker | embed (`gp_pick_embed`), plus the select menu and Start/Cancel row while `open` |
| `Prompt(GpWindowOpened)` | round prompt | embed (`gp_prompt_embed`) |
| `PromptClosed(GpWindowClosed)` | the prompt once the window closes | embed (`gp_prompt_closed_embed`) |
| `Song(GpTrackStart)` | the song message | embed (`gp_track_embed`), plus `gp_components(..)` |
| `Reveal(GpTrackResult)` | the reveal | embed (`gp_reveal_embed`), no components |
| `RoundResults(GpRoundResult)` | round results | embed (`gp_round_results_embed`) |
| `Scoreboard { scores: Vec<(UserId, u32)>, title: &'static str, lead: Option<&'static str> }` | game over, `/gp end`, a lost game | embed (`gp_scoreboard_embed`); `lead` (`GP_LOST`) is the content |
| `Line(String)` | window warning, abort, lost, resumed, a vote's room line, a prefix vote's DM | plain text, exactly as today |

- `CrackedMessage::Gp(Box<GpCard>)` is the one new variant. `render()` gets one arm: `CrackedMessage::Gp(card) => crate::commands::music::gp::render_card(card, cx)`. It is not named `render`, because `gp/mod.rs` glob-re-exports `ui`. `messaging` already depends on `/gp` (`GpClip`, `GpReveal`), so this adds no new edge.
- `gp::ui::render_card(card, cx) -> Rendered` composes the embed, the components and the content. The `gp_*_embed` builders keep their signatures, so the existing `ui` tests keep working.
- Its `Display` (used for logs and `to_string`) is the card's kind, for example `"gp: prompt"`. `Line` displays its text.
- **The shared formatter runs inside the builders.** It applies to third-party text only, as `format::escape`'s own rule says.
  - **Song titles and URLs** go through `TrackLabel::linked(GP_TITLE_MAX)`: trimmed, `(untitled)` when blank, capped, then escaped. The link is shown only for an http(s) URL. That renders `[**title**](url)`, the same as today's `**[title](url)**`.
  - `GP_TITLE_MAX` is 100, YouTube's own title limit, so a real title is never cut. The music path's `INLINE_TITLE_MAX` (60) would cut ordinary titles in the results list.
  - **Player display names** in `/gp status` go through `escape`.
  - **Prompts** come from our bundled `gp_prompts.json` and are not escaped.
  - Every description is clipped to `DESCRIPTION_MAX` and every field value to `FIELD_MAX` with `format::clip`. The round results' own hand-written cut at 4096 becomes that `clip`.
- **Unchanged:** the `CrackedMessage::Gp*` variants that already exist (`GpStarted`, `GpSubmitted`, `GpRoundSkipped`, `GpEnded`, the vote confirmations). They already reach the courier through `send_reply` and `send_message`.

## 2. Delivery

`GpPlayback.http: Arc<Http>` becomes `transport: Arc<dyn Transport>`. It is built from `DiscordTransport::of(ctx)` in `gp_playback` (commands.rs) and in `gp_resume_guild` (gp_persist.rs). `http` is only used for sends today.

Every send keeps today's failure behaviour; only the wire changes:

| send site | today | on the courier |
|---|---|---|
| prompt and song post (`gp_send`) | retry once; the caller aborts the game if both attempts fail | `gp_send` stays, and retries over `courier::post_message` |
| close and reveal | edit in place, else post a new message, logging if that fails too | gp's own `edit_or_post` over `courier::edit_rendered_message`, then `courier::post` |
| round results (`gp_advance`) | `gp_mark_results_posted` only on success | `post_message`, then mark on `Ok` |
| final scoreboard (`gp_follow`), a vote's room line (`gp_answer_vote`) | `?`: the error reaches the caller | `post_message`, still `?` |
| window warning, abort, lost, resumed; owed results and scoreboards on resume | warn and continue | `courier::post(.., Destination::Channel(..), ..)`, which logs and swallows. Owed results need the posted list, so they use `post_message` |
| a prefix vote's DM | warn and continue | `post_message` to the DM channel |
| `/gp`, `/gp status`, `/gp end`'s scoreboard | `send_embed_response(raw embed)` | `courier::reply_as(ctx, CrackedMessage::Gp(..), false)` |

**`Transport::clear_components(channel, id)`** is the one new transport method. Resume takes the dropdown off the pre-restart song message and leaves its embed in place. `Transport::edit` replaces the whole message, so it would wipe the embed. `DiscordTransport` implements the new method as `EditMessage::new().components(vec![])`; `FakeTransport` records `Op::ClearComponents(channel, id)`.

`FakeTransport` also gains a queue of per-send results, so a test can fail the first send and let the second through (`gp_send`'s retry). Its single `send_error`, which fails every send, stays as it is.

## 3. Interactions

`/gp` answers three kinds of component click:

- the guess dropdown and 👍 on a song message (`handle_gp_component`, dispatched on `gp:`);
- the picker's select menu, Start and Cancel, answered by a collector in `gp_pick_categories`;
- a non-host's click on a public (prefix) picker.

`Press` is widened with the two responses `/gp` uses today:

```rust
/// The interaction's one response: a new message, ephemeral or not.
async fn respond(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError>;
/// The interaction's one response: redraw the message the component is on.
async fn update(&self, out: Rendered) -> Result<(), TransportError>;
```

- `DiscordPress` implements them with `CreateInteractionResponse::Message(out.to_interaction_message().ephemeral(..))` and `CreateInteractionResponse::UpdateMessage(out.to_interaction_message())`.
- `FakePress` records `PressOp::Respond { ephemeral, text }` and `PressOp::Update { text, components }`, where `components` is the number of rows.
- `courier::respond(press, msg, cx, ephemeral)` and `courier::update(press, msg, cx)` render and return the transport's error, as `/gp`'s `?` does today.

**Why not acknowledge first, as the now-playing buttons do:** `/gp`'s answer comes from in-memory game state and the cache, with no I/O, so it is far inside Discord's three seconds. One response keeps one round trip and leaves the players' experience unchanged. The now-playing buttons acknowledge first because their ops take locks and can be slow.

- **`handle_gp_component`** works out the answer text exactly as today: the same checks in the same order, and the same outcomes. It then hands the text to `gp_answer_component(press: &dyn Press, text)`, which makes one `courier::respond(.., GpCard::Line(text), .., true)`. The real handler passes a `DiscordPress`, and tests pass a `FakePress`.
- **The picker:**
  - Its first post is `courier::reply_as(ctx, Gp(Picker { open: true, .. }), true)`. The message id still comes from `ReplyHandle::message`.
  - A non-host's click gets `courier::respond(.., Line(GP_PICK_NOT_HOST), true)`.
  - A select, Start or Cancel gets `courier::update(.., Gp(Picker { .. }))`.
  - On timeout the reply is edited, and its components must go. The courier's reply edit leaves components alone, because poise's builder cannot take `'static` components (ruling R4). An *empty* list has no lifetime, so `ReplySink` gains `retire(handle, out)`: an edit that also clears the components. `PoiseReplies` implements it with `.components(vec![])`, `FakeReplies` records `ReplyOp::Retire`, and `courier::retire_reply(ctx, handle, msg)` calls it.

The collector loop itself stays untested by unit tests (`cfg(not(tarpaulin_include))`), as today. The cards it renders and the press operations it makes are tested.

## 4. Enforcement, tests, release

**Enforcement.** All 22 "messaging arc: not migrated yet" expects in `gp/` and `gp_persist.rs` are deleted. An `#[expect]` that no longer fires is itself a clippy error, so none can be left behind by accident. `clippy.toml` needs no change, because it already bans every method these files call. The queue-ops expects (`stop_queue`, `force_skip_top_track`, the clip timer, `Songbird::get` in `/gp end`) stay: they are a different rule.

**Tests** (each one sabotaged once, in a mutation → caught-by table in the PR body):

- **Cards:** `gp::ui::render_card` for every `GpCard` variant:
  - `Song` carries its component rows, and `Reveal` carries none;
  - `Picker { open: false }` carries no rows;
  - `Line` is content with no embed;
  - `Scoreboard`'s `lead` is the content.
- **Formatter:**
  - a title with `*`, `_` and backticks comes out escaped in the song, reveal and round-results embeds;
  - a 300-character title is capped at `GP_TITLE_MAX` with `…` on a character boundary, and a 100-character title is not cut;
  - a round of 25 long titles keeps the results description within 4096.
- **Delivery,** against `FakeTransport` with a `GpPlayback` built over the test `Data` and `standalone_call`:
  - a round opens: the prompt is posted, and its id is recorded as the prompt message;
  - the prompt send fails once: it is retried and the game goes on;
  - the prompt send fails twice: the abort line is posted and the game is removed;
  - the close edits the prompt in place, and falls back to a post when the edit fails;
  - the reveal edits the song message with the reveal and no components;
  - a round's last reveal posts the results and marks them, and a failed results post leaves them unmarked;
  - the game's last reveal posts the final scoreboard;
  - the window warning is posted as a line.
- **Interactions,** against `FakePress`:
  - `gp_answer_component` makes exactly one `Respond { ephemeral: true }` with the text, and no acknowledge (the outcomes themselves are already tested through `gp_toggle_like` and the guess tests);
  - `courier::update` sends `Update` with the picker's rows;
  - `retire_reply` records `Retire`.
- **Resume,** against `FakeTransport`: owed results are posted and the posted rounds are returned; a resumed song clears the old message's components with `ClearComponents` and does not edit its embed.
- **Transport:** `DiscordTransport::clear_components` and the new `DiscordPress` methods are thin wrappers that only the TuneTitan check exercises, the same as the existing ones.

**Release.** A patch: **v0.24.1**. There is no new feature; the visible change is the title formatting fix. CHANGELOG entry under 0.24.1. The PR gets a TuneTitan checklist:

1. `/gp start` with the picker: tick categories, Start; again with Cancel; again left to time out, after which the buttons are gone.
2. A prefix `!gp start`: a second user's click on the picker is refused privately.
3. A full round: the prompt, the 30 s warning, the close edit, then each song with its dropdown and 👍. Guess and like answers stay private. The reveal replaces the dropdown, then the results, then the final scoreboard.
4. A song whose title has markdown in it (for example `*NSYNC`) shows the asterisk.
5. `/gp voteskip` from slash, and from prefix (a DM plus the room line).
6. Restart the bot mid-song: the resumed line is posted, the old message loses its dropdown and keeps its embed, and the song restarts.
7. `/gp status`, `/gp end` (with its scoreboard), and `/gp` (the rules).
