# `/gp` on the courier Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every `/gp` message is rendered by `messaging::render` and delivered through `messaging::courier`. The 22 "messaging arc: not migrated yet" expects in `gp/` and `gp_persist.rs` are gone, and `/gp`'s Discord glue is tested against the fakes.

**Architecture:**
- One new `CrackedMessage::Gp(Box<GpCard>)` variant. `GpCard` is a gp-owned enum in `gp/ui.rs`, rendered by `gp::render_card`, which `render()` dispatches to.
- `GpPlayback` carries an `Arc<dyn Transport>` instead of `Arc<Http>`.
- `Transport` gains `clear_components`; `Press` gains `respond` and `update`; `ReplySink` gains `retire`. Those are the three responses `/gp` uses that the courier lacks today.

**Tech Stack:** Rust (stable), serenity `next`, poise, songbird; tokio tests with `test-util`.

**Spec:** `docs/superpowers/specs/2026-10-09-gp-on-the-courier-design.md`. Read it first: it has the rulings, and this plan argues from it.

## Global Constraints

- **A pure move.** No change to game rules, scoring, timings, or the order of any check. #469 and #423 are NOT fixed here, even when you are editing their lines.
- **Each send keeps today's failure behaviour** (spec section 2): `gp_send` retries once and its callers abort; `?` sites stay `?`; warn-and-continue sites stay best effort; results are marked posted only on a successful post.
- **The only visible change** is the formatter:
  - song titles go through `TrackLabel::linked(GP_TITLE_MAX)` or `TrackLabel::title_text(GP_TITLE_MAX)`, with `GP_TITLE_MAX = 100`;
  - player display names in `/gp status` go through `format::escape`;
  - prompts are NOT escaped;
  - descriptions are clipped with `format::clip(.., DESCRIPTION_MAX)`, and list fields with `format::clip(.., FIELD_MAX)`.
- **Strings stay in `crack-core/src/messaging/messages.rs`** (#537). Don't inline new user-facing text. The `Display` kind names for `GpCard` (`"gp: prompt"`, …) are log text, not user text.
- **Typed serde only.** Never `serde_json::json!` or `Value` for data we own. Tests may `serde_json::to_value(embed)` to inspect a serenity builder, as `gp/test.rs` already does.
- **Never `git add -A`.** Add the files you changed by path.
- **Commit trailer:** every commit ends with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other co-author line.
- **Gates:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return`, `cargo test -p crack-core --lib`. Run each before every commit. `--all-targets` matters: the tests are lint-checked too.
- **Sabotage every new test once.** Break the line it guards, watch it fail, then restore. Report each one as `mutation → test that caught it` in your task report.
- **Shell:** the Bash tool runs zsh, and `for f in $files` does not word-split. Use `bash -c '…'` with arrays for any loop.

## Review Focus

1. **A title made of markdown** (`*NSYNC`, `__init__`, `` `rm -rf` ``, `@everyone`): the song, reveal and results embeds must show it literally, not formatted, and must not ping. Task 2 pins it with `a_title_with_markdown_shows_literally_everywhere`.
2. **A song message whose edit fails** (deleted by `/clean`, or by hand) before the reveal: the reveal must still reach the channel as a new message, or the room never learns whose song it was. Task 3 pins it with `a_reveal_whose_edit_fails_is_posted_instead`.
3. **The results post fails** (rate limit, a 5xx): the round must stay unmarked, so the next resume posts it, and the game must still move on to the next round. Task 3 pins it with `a_failed_results_post_leaves_the_round_owed_and_the_game_moves_on`.
4. **A bot restart mid-song:** the pre-restart song message must lose its dropdown and KEEP its embed. `Transport::edit` would wipe the embed, so this is the trap `clear_components` exists for. Task 5 pins it with `taking_down_the_old_dropdown_clears_components_and_nothing_else`.
5. **A picker left to time out:** its Start/Cancel buttons must disappear. The courier's ordinary reply edit leaves components in place (ruling R4), so the timeout must go through `retire`. Task 1 pins it with `retiring_a_reply_edits_it_and_clears_its_components`; the picker uses it in Task 4.

---

### Task 1: Widen the messaging seams `/gp` needs

**Files:**
- Modify: `crack-core/src/messaging/transport.rs` (`TransportError`, `Transport`, `DiscordTransport`, `Press`, `DiscordPress`)
- Modify: `crack-core/src/messaging/courier.rs` (`ReplySink`, `PoiseReplies`, new `respond`/`update`/`retire_reply_on`/`retire_reply`, tests)
- Modify: `crack-core/src/messaging/test_support.rs` (`FakeTransport`, `FakeReplies`, `FakePress`)

**Interfaces:**
- Produces:
  - `impl std::fmt::Display for TransportError` and `impl std::error::Error for TransportError`, so `?` converts it into `crate::Error`;
  - `Transport::clear_components(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError>`;
  - `Press::respond(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError>` and `Press::update(&self, out: Rendered) -> Result<(), TransportError>`;
  - `ReplySink::retire(&self, handle: &Self::Handle, out: Rendered) -> Result<(), CrackedError>`;
  - `courier::respond(press: &dyn Press, msg: &CrackedMessage, cx: &RenderCx, ephemeral: bool) -> Result<(), TransportError>`;
  - `courier::update(press: &dyn Press, msg: &CrackedMessage, cx: &RenderCx) -> Result<(), TransportError>`;
  - `courier::retire_reply_on<S: ReplySink>(sink: &S, handle: &S::Handle, msg: &CrackedMessage, cx: &RenderCx) -> Result<(), CrackedError>`;
  - `courier::retire_reply<'ctx>(ctx: crate::Context<'ctx>, handle: &poise::ReplyHandle<'ctx>, msg: CrackedMessage) -> Result<(), CrackedError>`.
- Produces (fakes):
  - `Op::ClearComponents(u64, u64)`;
  - `FakeTransport::send_failures: Mutex<VecDeque<TransportError>>`: each `send` pops one; when the queue is empty it falls back to `send_error`;
  - `PressOp::Respond { ephemeral: bool, text: String }`, `PressOp::Update { text: String, rows: usize }`;
  - `ReplyOp::Retire { handle: u64, text: String }`.

- [ ] **Step 1: Write the failing tests** in `courier.rs`'s `mod tests`, next to `a_press_is_acknowledged_then_answered_privately`:

```rust
    #[tokio::test]
    async fn a_press_answered_now_is_one_private_response() {
        let press = FakePress::default();
        respond(&press, &CrackedMessage::Other("ok".into()), &cx(), true)
            .await
            .unwrap();
        assert_eq!(
            press.ops(),
            vec![PressOp::Respond {
                ephemeral: true,
                text: "ok".into()
            }]
        );
    }

    #[tokio::test]
    async fn an_update_redraws_the_message_with_its_rows() {
        let press = FakePress::default();
        let msg = CrackedMessage::Other("page".into());
        // `render` gives an `Other` no components; give it two rows to see them counted.
        let out = render(&msg, &cx()).with_components(vec![
            serenity::all::CreateComponent::ActionRow(serenity::all::CreateActionRow::Buttons(
                std::borrow::Cow::Owned(vec![serenity::all::CreateButton::new("a")]),
            )),
            serenity::all::CreateComponent::ActionRow(serenity::all::CreateActionRow::Buttons(
                std::borrow::Cow::Owned(vec![serenity::all::CreateButton::new("b")]),
            )),
        ]);
        press.update(out).await.unwrap();
        update(&press, &msg, &cx()).await.unwrap();
        assert_eq!(
            press.ops(),
            vec![
                PressOp::Update {
                    text: "page".into(),
                    rows: 2
                },
                PressOp::Update {
                    text: "page".into(),
                    rows: 0
                },
            ]
        );
    }

    #[tokio::test]
    async fn retiring_a_reply_edits_it_and_clears_its_components() {
        let sink = FakeReplies::default();
        let handle = reply_on(&sink, &CrackedMessage::Other("pick".into()), &cx(), true)
            .await
            .unwrap();
        retire_reply_on(&sink, &handle, &CrackedMessage::Other("timed out".into()), &cx())
            .await
            .unwrap();
        assert_eq!(
            sink.ops().last().unwrap(),
            &ReplyOp::Retire {
                handle,
                text: "timed out".into()
            }
        );
    }

    #[tokio::test]
    async fn clearing_components_touches_only_the_components() {
        let t = FakeTransport::default();
        t.clear_components(GenericChannelId::new(5), MessageId::new(9))
            .await
            .unwrap();
        assert_eq!(t.ops(), vec![Op::ClearComponents(5, 9)]);
        assert!(t.sent.lock().unwrap().is_empty(), "nothing was re-rendered");
    }

    #[tokio::test]
    async fn queued_send_failures_fail_that_many_sends_then_let_them_through() {
        let t = FakeTransport::default();
        t.send_failures
            .lock()
            .unwrap()
            .push_back(TransportError::Other("503".into()));
        let first = t
            .send(GenericChannelId::new(5), Rendered::text("a"))
            .await;
        let second = t
            .send(GenericChannelId::new(5), Rendered::text("a"))
            .await;
        assert_eq!(first, Err(TransportError::Other("503".into())));
        assert_eq!(second, Ok(MessageId::new(1000)));
    }

    #[test]
    fn a_transport_error_reads_as_text() {
        assert_eq!(TransportError::UnknownMessage.to_string(), "unknown message");
        assert_eq!(TransportError::Other("boom".into()).to_string(), "boom");
    }
```

Add `PressOp`, `ReplyOp`, `Op` and `Rendered` to the test module's `use` lines if they are not already there.

- [ ] **Step 2: Run them and watch them fail to compile**

Run: `cargo test -p crack-core --lib messaging::courier`
Expected: compile errors: no `respond`, `update`, `retire_reply_on`, `clear_components`, `send_failures`, `PressOp::Respond`, or `Display` for `TransportError`.

- [ ] **Step 3: Implement.** In `transport.rs`:

```rust
impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownMessage => f.write_str("unknown message"),
            Self::Other(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for TransportError {}
```

Add to `trait Transport`, after `delete`:

```rust
    /// Take the components off a message and leave the rest of it as it is.
    /// `edit` cannot: it replaces the whole message, embed included.
    async fn clear_components(
        &self,
        channel: GenericChannelId,
        id: MessageId,
    ) -> Result<(), TransportError>;
```

and to `impl Transport for DiscordTransport`:

```rust
    async fn clear_components(
        &self,
        channel: GenericChannelId,
        id: MessageId,
    ) -> Result<(), TransportError> {
        #[expect(
            clippy::disallowed_methods,
            reason = "messaging is where sends are made"
        )]
        channel
            .edit_message(
                &self.http,
                id,
                serenity::all::EditMessage::new()
                    .components(Vec::<serenity::all::CreateComponent<'_>>::new()),
            )
            .await?;
        Ok(())
    }
```

Add to `trait Press`:

```rust
    /// The interaction's one response: a new message, ephemeral or not. For
    /// an answer worked out from memory, well inside Discord's three seconds.
    async fn respond(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError>;
    /// The interaction's one response: redraw the message the component is on.
    async fn update(&self, out: Rendered) -> Result<(), TransportError>;
```

and to `impl Press for DiscordPress<'_>`:

```rust
    #[expect(
        clippy::disallowed_methods,
        reason = "messaging is where sends are made"
    )]
    async fn respond(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError> {
        Ok(self
            .interaction
            .create_response(
                self.http,
                CreateInteractionResponse::Message(
                    out.to_interaction_message().ephemeral(ephemeral),
                ),
            )
            .await?)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "messaging is where sends are made"
    )]
    async fn update(&self, out: Rendered) -> Result<(), TransportError> {
        Ok(self
            .interaction
            .create_response(
                self.http,
                CreateInteractionResponse::UpdateMessage(out.to_interaction_message()),
            )
            .await?)
    }
```

In `courier.rs`, add to `trait ReplySink`:

```rust
    /// An edit that also takes the reply's components away: a menu that has
    /// closed. `edit` cannot (ruling R4); an empty list has no lifetime to fight.
    async fn retire(&self, handle: &Self::Handle, out: Rendered) -> Result<(), CrackedError>;
```

Implement it for `PoiseReplies`:

```rust
    async fn retire(&self, handle: &Self::Handle, out: Rendered) -> Result<(), CrackedError> {
        retire_poise(self.0, handle, out).await
    }
```

with, next to `edit_poise`:

```rust
#[expect(
    clippy::disallowed_methods,
    reason = "messaging is where sends are made"
)]
async fn retire_poise<'ctx>(
    ctx: crate::Context<'ctx>,
    handle: &poise::ReplyHandle<'ctx>,
    out: Rendered,
) -> Result<(), CrackedError> {
    let (reply, _) = out.to_reply_edit();
    let reply = reply.components(Vec::<serenity::all::CreateComponent<'_>>::new());
    handle.edit(ctx, reply).await.map_err(Into::into)
}
```

Then the courier functions, after `answer_privately`:

```rust
/// Answer a press with its one response, to the presser only when
/// `ephemeral`. Fallible: the caller decides whether a lost answer matters.
pub async fn respond(
    press: &dyn Press,
    msg: &CrackedMessage,
    cx: &RenderCx,
    ephemeral: bool,
) -> Result<(), TransportError> {
    press.respond(render(msg, cx), ephemeral).await
}

/// Answer a press by redrawing the message it came from. Fallible.
pub async fn update(
    press: &dyn Press,
    msg: &CrackedMessage,
    cx: &RenderCx,
) -> Result<(), TransportError> {
    press.update(render(msg, cx)).await
}

pub async fn retire_reply_on<S: ReplySink>(
    sink: &S,
    handle: &S::Handle,
    msg: &CrackedMessage,
    cx: &RenderCx,
) -> Result<(), CrackedError> {
    sink.retire(handle, render(msg, cx)).await
}

/// Edit a reply for the last time and take its components away.
pub async fn retire_reply<'ctx>(
    ctx: crate::Context<'ctx>,
    handle: &poise::ReplyHandle<'ctx>,
    msg: CrackedMessage,
) -> Result<(), CrackedError> {
    retire_reply_on(&PoiseReplies(ctx), handle, &msg, &RenderCx::now()).await
}
```

In `test_support.rs`:
- add `ClearComponents(u64, u64)` to `Op`;
- add `pub send_failures: Mutex<std::collections::VecDeque<TransportError>>` to `FakeTransport`;
- in `send`, after recording the op and the rendered message, and before the `send_error` check, add:

```rust
        if let Some(err) = self.send_failures.lock().unwrap().pop_front() {
            return Err(err);
        }
```

Implement `clear_components` on `FakeTransport`. It records the op and answers from `edit_error`, because it is an edit as far as Discord is concerned:

```rust
    async fn clear_components(
        &self,
        channel: GenericChannelId,
        id: MessageId,
    ) -> Result<(), TransportError> {
        self.ops
            .lock()
            .unwrap()
            .push(Op::ClearComponents(channel.get(), id.get()));
        match self.edit_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
```

Add `Retire { handle: u64, text: String }` to `ReplyOp`, and to `impl ReplySink for FakeReplies`:

```rust
    async fn retire(&self, handle: &u64, out: Rendered) -> Result<(), CrackedError> {
        self.ops.lock().unwrap().push(ReplyOp::Retire {
            handle: *handle,
            text: text_of(&out),
        });
        self.sent.lock().unwrap().push(out);
        Ok(())
    }
```

Add `Respond { ephemeral: bool, text: String }` and `Update { text: String, rows: usize }` to `PressOp`, and to `impl Press for FakePress`:

```rust
    async fn respond(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(PressOp::Respond {
            ephemeral,
            text: text_of(&out),
        });
        Ok(())
    }
    async fn update(&self, out: Rendered) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(PressOp::Update {
            text: text_of(&out),
            rows: out.components.len(),
        });
        Ok(())
    }
```

If any existing `match` over `Op`, `PressOp` or `ReplyOp` stops compiling because it is exhaustive, add the new arms there the same way the existing ones are written.

- [ ] **Step 4: Run the tests and the gates**

Run: `cargo test -p crack-core --lib messaging` → all pass, including the six new ones.
Run: `cargo clippy --workspace --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return` → clean.
Run: `cargo fmt --all -- --check` → clean.

- [ ] **Step 5: Sabotage each new test once**
- `FakePress::respond` records `ephemeral: !ephemeral` → `a_press_answered_now_is_one_private_response` fails.
- `retire_reply_on` calls `sink.edit` → the retire test fails.
- `send` ignores `send_failures` → the queued-failure test fails.
- `FakeTransport::clear_components` pushes `Op::Edit` → the clear test fails.

Restore after each one, and record the four lines for the report.

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/messaging/transport.rs crack-core/src/messaging/courier.rs crack-core/src/messaging/test_support.rs
git commit -m "messaging: the seams /gp needs -- clear_components, Press::respond/update, ReplySink::retire

/gp answers a click with one immediate response and redraws its picker in
place; the courier had neither. Resume takes the dropdown off a song message
and keeps its embed, which Transport::edit cannot do. A timed-out picker
must lose its buttons, which a reply edit cannot do (ruling R4).
TransportError gains Display and Error, so ? carries it.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: `GpCard`, `render_card`, and the shared formatter

**Files:**
- Modify: `crack-core/src/commands/music/gp/ui.rs` (`GpCard`, `render_card`, `gp_rendered`, `GP_TITLE_MAX`, formatter in the builders)
- Modify: `crack-core/src/messaging/message.rs` (variant `Gp(Box<GpCard>)`, `Display` arm)
- Modify: `crack-core/src/messaging/render.rs` (one arm in `render`)
- Modify: `crack-core/src/commands/music/gp/test.rs` (card and formatter tests; the round-results test switches to `DESCRIPTION_MAX`)

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces (all re-exported from `crate::commands::music::gp` by its `pub use ui::*`):

```rust
pub const GP_TITLE_MAX: usize = 100;

#[derive(Debug, Clone)]
pub enum GpCard {
    Rules,
    Status(GpStatus),
    Picker { text: String, picked: Vec<GpCategory>, open: bool },
    Prompt(GpWindowOpened),
    PromptClosed(GpWindowClosed),
    Song { start: GpTrackStart, guild: GuildId },
    Reveal(GpTrackResult),
    RoundResults(GpRoundResult),
    Scoreboard { scores: Vec<(UserId, u32)>, title: &'static str, lead: Option<&'static str> },
    Line(String),
}
impl From<GpCard> for CrackedMessage { /* CrackedMessage::Gp(Box::new(card)) */ }
impl std::fmt::Display for GpCard { /* Line -> its text; others -> "gp: <kind>" */ }
pub fn render_card(card: &GpCard, cx: &RenderCx) -> Rendered;
/// `render(&card.into(), &RenderCx::now())`: the one way /gp turns a card into a body.
pub fn gp_rendered(card: GpCard) -> Rendered;
```

Note: `Song` carries the guild because `GpTrackStart` has no guild id, and `gp_components` needs one. The spec's table writes it as `Song(GpTrackStart)`; this shape is that plus the guild.

- [ ] **Step 1: Write the failing tests** at the end of `gp/test.rs`. They import `crate::messaging::render::{render, RenderCx, Rendered}` and `crate::messaging::format::DESCRIPTION_MAX`; reuse the file's existing helpers (`data`, `game_with`, `game_with_reveal`, `submit`, `rng`, the `G`, `TC`, `A`, `B` and `NOW` constants):

```rust
fn cx() -> RenderCx {
    RenderCx {
        now_unix: NOW,
        embed_links: true,
    }
}

fn embed_json(r: &Rendered) -> serde_json::Value {
    serde_json::to_value(r.embed.clone().expect("an embed")).unwrap()
}

fn description(r: &Rendered) -> String {
    embed_json(r)["description"].as_str().unwrap_or_default().to_string()
}

/// A one-round game whose only song is `title`, closed and so playing.
fn playing_with_title(data: &Data, title: &str, reveal: GpReveal) -> GpTrackStart {
    let opened = game_with_reveal(data, &["only"], None, reveal);
    submit(data, B, "bob", title);
    let closed = data
        .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    match closed.next {
        GpNext::Track(start) => *start,
        other => panic!("expected a song, got {other:?}"),
    }
}

#[test]
fn every_card_renders_through_the_one_renderer() {
    let line: CrackedMessage = GpCard::Line("hello".into()).into();
    let r = render(&line, &cx());
    assert_eq!(r.content.as_deref(), Some("hello"));
    assert!(r.embed.is_none(), "a line is plain text, as /gp sends it today");
    assert_eq!(line.to_string(), "hello");
}

#[test]
fn a_song_carries_its_controls_and_a_reveal_carries_none() {
    let data = data();
    let start = playing_with_title(&data, "Song", GpReveal::Song);
    let song = render_card(&GpCard::Song { start: start.clone(), guild: G }, &cx());
    assert!(!song.components.is_empty(), "the 👍 row at least");
    let res = data
        .gp_reveal_and_advance(G, 0, 0, NOW)
        .expect("the song was playing");
    let reveal = render_card(&GpCard::Reveal(res), &cx());
    assert!(reveal.components.is_empty(), "the reveal takes the dropdown away");
    assert!(reveal.embed.is_some());
}

#[test]
fn the_picker_has_its_rows_only_while_open() {
    let open = render_card(
        &GpCard::Picker {
            text: "pick".into(),
            picked: vec![],
            open: true,
        },
        &cx(),
    );
    let closed = render_card(
        &GpCard::Picker {
            text: "done".into(),
            picked: vec![],
            open: false,
        },
        &cx(),
    );
    assert_eq!(open.components.len(), 2);
    assert!(closed.components.is_empty());
}

#[test]
fn a_lost_games_scoreboard_leads_with_the_line() {
    let r = render_card(
        &GpCard::Scoreboard {
            scores: vec![(A, 3)],
            title: GP_SCOREBOARD,
            lead: Some(GP_LOST),
        },
        &cx(),
    );
    assert_eq!(r.content.as_deref(), Some(GP_LOST));
    assert_eq!(embed_json(&r)["title"], GP_SCOREBOARD);
}

#[test]
fn a_title_with_markdown_shows_literally_everywhere() {
    let raw = "*NSYNC_`x` @everyone";
    let escaped = "\\*NSYNC\\_\\`x\\` \\@everyone";
    let data = data();
    let start = playing_with_title(&data, raw, GpReveal::Round);
    let song = render_card(&GpCard::Song { start, guild: G }, &cx());
    assert!(description(&song).contains(escaped), "song: {}", description(&song));
    let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    let reveal = render_card(&GpCard::Reveal(res.clone()), &cx());
    assert!(description(&reveal).contains(escaped), "reveal: {}", description(&reveal));
    let results = render_card(
        &GpCard::RoundResults(res.round.expect("the round's last song posts results")),
        &cx(),
    );
    assert!(description(&results).contains(escaped), "results: {}", description(&results));
}

#[test]
fn a_long_title_is_capped_and_a_real_one_is_not() {
    let data = data();
    let long = "a".repeat(300);
    let start = playing_with_title(&data, &long, GpReveal::Song);
    let d = description(&render_card(&GpCard::Song { start, guild: G }, &cx()));
    // The test track's URL carries the title too, so look at the link text only.
    assert!(d.contains(&format!("[**{}…**]", "a".repeat(GP_TITLE_MAX))), "{d}");

    let data = self::data();
    let real = "b".repeat(GP_TITLE_MAX);
    let start = playing_with_title(&data, &real, GpReveal::Song);
    let d = description(&render_card(&GpCard::Song { start, guild: G }, &cx()));
    assert!(d.contains(&format!("[**{real}**]")), "{d}");
}
```

`gp_reveal_and_advance` is called the way `gp_advance_track` calls it (`Data::gp_reveal_and_advance(&data, guild, round, track, now)`); check its exact signature in `state.rs` and adjust the call if it differs. If `GpNext` does not derive `Debug`, change the panic to `panic!("expected a song")`.

Also update the existing round-results length test (the one asserting `desc.chars().count() <= GP_EMBED_DESCRIPTION_MAX`, around line 1863) to use `DESCRIPTION_MAX`.

- [ ] **Step 2: Run them and watch them fail to compile**

Run: `cargo test -p crack-core --lib commands::music::gp`
Expected: compile errors: no `GpCard`, `render_card` or `GP_TITLE_MAX`.

- [ ] **Step 3: Implement `GpCard` and `render_card`** in `gp/ui.rs`, after the imports:

```rust
/// A submitted song's title is third-party text; YouTube caps titles at 100,
/// so a real one is never cut and only a pathological one is.
pub const GP_TITLE_MAX: usize = 100;

/// Every message `/gp` sends, as data. [`render_card`] turns one into Discord
/// output, and `messaging::render` reaches it through `CrackedMessage::Gp`, so
/// `/gp` has no way to Discord but the one renderer.
#[derive(Debug, Clone)]
pub enum GpCard {
    Rules,
    Status(GpStatus),
    /// The category picker; its menu and Start/Cancel only while `open`.
    Picker {
        text: String,
        picked: Vec<GpCategory>,
        open: bool,
    },
    Prompt(GpWindowOpened),
    PromptClosed(GpWindowClosed),
    /// The song message and its guess/👍 controls. `GpTrackStart` has no guild.
    Song {
        start: GpTrackStart,
        guild: GuildId,
    },
    Reveal(GpTrackResult),
    RoundResults(GpRoundResult),
    /// `lead` rides as content above the embed (a lost game's `GP_LOST`).
    Scoreboard {
        scores: Vec<(UserId, u32)>,
        title: &'static str,
        lead: Option<&'static str>,
    },
    /// Plain text, as `/gp` has always sent its one-liners.
    Line(String),
}

impl From<GpCard> for CrackedMessage {
    fn from(card: GpCard) -> Self {
        CrackedMessage::Gp(Box::new(card))
    }
}

/// For logs: a line is its text, everything else its kind.
impl std::fmt::Display for GpCard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::Line(text) => return f.write_str(text),
            Self::Rules => "rules",
            Self::Status(_) => "status",
            Self::Picker { .. } => "picker",
            Self::Prompt(_) => "prompt",
            Self::PromptClosed(_) => "prompt closed",
            Self::Song { .. } => "song",
            Self::Reveal(_) => "reveal",
            Self::RoundResults(_) => "round results",
            Self::Scoreboard { .. } => "scoreboard",
        };
        write!(f, "gp: {kind}")
    }
}

/// What a card looks like in Discord. Reached through `messaging::render`.
pub fn render_card(card: &GpCard, _cx: &RenderCx) -> Rendered {
    match card {
        GpCard::Rules => Rendered::embed(gp_rules_embed()),
        GpCard::Status(status) => Rendered::embed(gp_status_embed(status)),
        GpCard::Picker { text, picked, open } => {
            let out = Rendered::embed(gp_pick_embed(text));
            if *open {
                out.with_components(gp_pick_components(picked))
            } else {
                out
            }
        },
        GpCard::Prompt(w) => Rendered::embed(gp_prompt_embed(w)),
        GpCard::PromptClosed(c) => Rendered::embed(gp_prompt_closed_embed(c)),
        GpCard::Song { start, guild } => {
            Rendered::embed(gp_track_embed(start)).with_components(gp_components(
                *guild,
                start.round_idx,
                start.track_idx,
                &start.players,
                start.guessable,
            ))
        },
        GpCard::Reveal(res) => Rendered::embed(gp_reveal_embed(res)),
        GpCard::RoundResults(r) => Rendered::embed(gp_round_results_embed(r)),
        GpCard::Scoreboard {
            scores,
            title,
            lead,
        } => {
            let out = Rendered::embed(gp_scoreboard_embed(scores, title));
            match lead {
                Some(lead) => out.with_content(*lead),
                None => out,
            }
        },
        GpCard::Line(text) => Rendered::text(text.clone()),
    }
}

/// A card as a ready body, through the one renderer.
pub fn gp_rendered(card: GpCard) -> Rendered {
    render(&card.into(), &RenderCx::now())
}
```

Imports to add to `ui.rs`: `crate::messaging::{format::{clip, escape, TrackLabel, DESCRIPTION_MAX, FIELD_MAX}, message::CrackedMessage, render::{render, RenderCx, Rendered}}`.

In `messaging/message.rs`, append after `ButtonOutOfDate,` (the enum compares discriminants, so new variants go at the end):

```rust
    /// Every `/gp` message, rendered by `gp::render_card`.
    Gp(Box<crate::commands::music::gp::GpCard>),
```

and in `Display`, next to `Self::ButtonOutOfDate`:

```rust
            Self::Gp(card) => write!(f, "{card}"),
```

In `messaging/render.rs`'s `render`, next to the `Echo` arm:

```rust
        CrackedMessage::Gp(card) => crate::commands::music::gp::render_card(card, cx),
```

- [ ] **Step 4: Adopt the formatter in the builders** in `gp/ui.rs`. Add two helpers:

```rust
/// A submitted song as `[**title**](url)`: escaped, capped at
/// [`GP_TITLE_MAX`], and a link only for an http(s) URL.
fn song_link(title: &str, url: &str) -> String {
    TrackLabel {
        title: Some(title.to_owned()),
        url: Some(url.to_owned()),
        duration: None,
    }
    .linked(GP_TITLE_MAX)
}

/// A submitted song's title alone: escaped and capped.
fn song_name(title: &str) -> String {
    TrackLabel {
        title: Some(title.to_owned()),
        url: None,
        duration: None,
    }
    .title_text(GP_TITLE_MAX)
}
```

Then:
- **`gp_track_embed`:** the description becomes `clip(&format!("*{}*\n\n{}\n\n{hint}", s.prompt, song_link(&s.track.get_title(), &s.track.get_url())), DESCRIPTION_MAX)`.
- **`gp_reveal_embed`:** in all three branches, replace `**[{}]({})**` with `{}` fed by `song_link(&res.title, &res.url)`, and wrap each `.description(...)` argument in `clip(&…, DESCRIPTION_MAX)`. Wrap the `GP_GUESSED_RIGHT` field value in `clip(&correct, FIELD_MAX)` and each `scores_lines(..)` field value in `clip(&…, FIELD_MAX)`.
- **`song_result_line`:** `**{}**` with `s.title` becomes `**{}**` with `song_name(&s.title)`.
- **`gp_round_results_embed`:** delete the hand-written cut (the `chars().take(GP_EMBED_DESCRIPTION_MAX - 1)` block), and after the names → counts fallback set `description = clip(&description, DESCRIPTION_MAX);`. The fallback compares against `DESCRIPTION_MAX`. Delete `GP_EMBED_DESCRIPTION_MAX`. Clip the two field values with `FIELD_MAX`.
- **`gp_status_embed`:** in `list`, join `names.iter().map(|n| escape(n))` and wrap the result in `clip(&…, FIELD_MAX)`; wrap `scores_lines(scores)` in `clip(&…, FIELD_MAX)`.
- **`gp_scoreboard_embed`:** `.description(clip(&scores_lines(scores), DESCRIPTION_MAX))`.
- **`gp_prompt_closed_embed`:** wrap its description in `clip(&…, DESCRIPTION_MAX)`. Prompts are ours: do NOT escape `prompt_text`.

- [ ] **Step 5: Run the tests and the gates**

Run: `cargo test -p crack-core --lib` → all pass. Existing gp tests that compare a description containing `**[title](url)**` should still pass if they use `contains` on the title. If one asserts the old link shape exactly, update it to `[**title**](url)`: that is the agreed rendering, the same bold link. Note each such test in your report.
Run the clippy and fmt gates → clean. (Nothing calls `render_card` from production code until Task 3, so if clippy reports `gp_rendered` as dead code, add `#[cfg_attr(not(test), expect(dead_code, reason = "used from Task 3 on"))]` and remove it in Task 3. Because the module glob-re-exports `pub` items, clippy will most likely not complain at all.)

- [ ] **Step 6: Sabotage**
- `song_link` passes the raw title (no `TrackLabel`) → `a_title_with_markdown_shows_literally_everywhere` fails.
- `GP_TITLE_MAX` is 60 → `a_long_title_is_capped_and_a_real_one_is_not` fails, on its second half.
- `render_card`'s `Reveal` arm adds `gp_components(..)` → the reveal test fails.
- `Line` renders as `Rendered::embed` → `every_card_renders_through_the_one_renderer` fails.

Restore after each one.

- [ ] **Step 7: Commit**

```bash
git add crack-core/src/commands/music/gp/ui.rs crack-core/src/commands/music/gp/test.rs crack-core/src/messaging/message.rs crack-core/src/messaging/render.rs
git commit -m "gp: every message is a GpCard, rendered through the one renderer

CrackedMessage::Gp(Box<GpCard>) reaches gp::render_card from render().
Song titles adopt the shared formatter: escaped, capped at 100 (YouTube's
own limit, so a real title is never cut), linked only for http(s).
Player names in /gp status are escaped; prompts are ours and are not.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: Playback on the courier

**Files:**
- Modify: `crack-core/src/commands/music/gp/playback.rs` (`GpPlayback`, `gp_send`, `gp_abort`, the window timer, `gp_after_close`, `gp_follow`, `gp_advance_track`, `handle_gp_component`; new `gp_post`, `gp_edit_or_post`, `gp_answer_component`)
- Modify: `crack-core/src/commands/music/gp/commands.rs:87-94` (`gp_playback` builds a transport)
- Modify: `crack-core/src/commands/music/gp_persist.rs` (the `GpPlayback { .. }` literal only: `transport: Arc::new(DiscordTransport::of(ctx))`; the rest of the file is Task 5)
- Create: `crack-core/src/commands/music/gp/test_delivery.rs`
- Modify: `crack-core/src/commands/music/gp/mod.rs` (add `#[cfg(test)] mod test_delivery;`)
- Modify: `crack-core/src/commands/music/gp/test.rs` (make the helpers and constants the new file uses `pub(super)`)
- Modify: `crack-core/Cargo.toml` (`[dev-dependencies]`: `tokio = { workspace = true, features = ["test-util"] }`)

**Interfaces:**
- Consumes:
  - from Task 1: `courier::{post, post_message, edit_message, respond}`, `Destination::Channel`, `Transport`, `DiscordTransport::of`, `DiscordPress { http, interaction }`, `FakeTransport { send_failures, edit_error }`, `FakePress`, `PressOp::Respond`;
  - from Task 2: `GpCard`, `gp_rendered`, `From<GpCard> for CrackedMessage`.
- Produces:
  - `pub struct GpPlayback { pub data: Arc<Data>, pub transport: Arc<dyn Transport>, pub call: Arc<Mutex<Call>>, pub guild_id: GuildId }`;
  - `pub async fn gp_post(data: &Data, transport: &dyn Transport, channel: GenericChannelId, card: GpCard)`, best effort;
  - `pub(in crate::commands::music::gp) async fn gp_answer_component(press: &dyn Press, text: String) -> Result<(), Error>`.
  Task 5 uses `gp_post`.

- [ ] **Step 1: Add the dev-dependency, expose the test helpers, and write the failing tests.**

In `crack-core/Cargo.toml` under `[dev-dependencies]` add:

```toml
# `#[tokio::test(start_paused = true)]`: /gp's reveal pauses five seconds.
tokio = { workspace = true, features = ["test-util"] }
```

In `gp/test.rs`, change `fn data`, `fn rng`, `fn game_with`, `fn game_with_reveal`, `fn game_with_settings`, `fn submit` and `fn game`, and the constants `G`, `TC`, `A`, `B` and `NOW`, from private to `pub(super)`.

Create `gp/test_delivery.rs`:

```rust
//! /gp's Discord glue against the messaging fakes: what each step of a game
//! sends, edits and gives up on. The game's rules are tested in `test.rs`.
use super::playback::{gp_after_close, gp_answer_component, gp_spawn_window_timer_secs};
use super::test::{data, game, game_with, game_with_reveal, rng, submit, A, B, G, NOW, TC};
use super::*;
use crate::messaging::messages::{GP_ABORTED, GP_GAME_OVER, GP_WINDOW_WARNING};
use crate::messaging::test_support::{FakePress, FakeTransport, Op, PressOp};
use crate::messaging::transport::TransportError;
use crate::music::ops::test_support::standalone_call;
use crate::Data;
use ::serenity::all::MessageId;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

fn playback(data: &Data, fake: &Arc<FakeTransport>) -> GpPlayback {
    GpPlayback {
        data: Arc::new(data.clone()),
        transport: fake.clone(),
        call: Arc::new(Mutex::new(standalone_call(G, A))),
        guild_id: G,
    }
}

fn fail_sends(fake: &FakeTransport, n: usize) {
    let mut q = fake.send_failures.lock().unwrap();
    for _ in 0..n {
        q.push_back(TransportError::Other("503".into()));
    }
}

fn embed_title(fake: &FakeTransport, i: usize) -> String {
    let sent = fake.sent.lock().unwrap();
    let e = serde_json::to_value(sent[i].embed.clone().expect("an embed")).unwrap();
    e["title"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn a_round_opens_by_posting_its_prompt_and_recording_where() {
    let data = data();
    let opened = game_with(&data, &["first"]);
    let fake = Arc::new(FakeTransport::default());
    gp_open_round(&playback(&data, &fake), opened).await.unwrap();
    assert_eq!(fake.ops(), vec![Op::Send(TC.get())]);
    assert!(fake.sent.lock().unwrap()[0].components.is_empty());
    assert_eq!(
        game(&data).rounds[0].prompt_message,
        Some((TC, MessageId::new(1000)))
    );
}

#[tokio::test]
async fn a_prompt_that_fails_once_is_sent_again() {
    let data = data();
    let opened = game_with(&data, &["first"]);
    let fake = Arc::new(FakeTransport::default());
    fail_sends(&fake, 1);
    gp_open_round(&playback(&data, &fake), opened).await.unwrap();
    assert_eq!(fake.ops(), vec![Op::Send(TC.get()), Op::Send(TC.get())]);
    assert!(data.gp_is_active(G));
    assert_eq!(
        game(&data).rounds[0].prompt_message,
        Some((TC, MessageId::new(1000)))
    );
}

#[tokio::test]
async fn a_prompt_that_fails_twice_ends_the_game_and_says_so() {
    let data = data();
    let opened = game_with(&data, &["first"]);
    let fake = Arc::new(FakeTransport::default());
    fail_sends(&fake, 2);
    gp_open_round(&playback(&data, &fake), opened).await.unwrap();
    assert!(!data.gp_is_active(G), "the game is discarded");
    assert_eq!(fake.ops().len(), 3);
    assert_eq!(fake.texts().last().unwrap(), GP_ABORTED);
    assert!(fake.sent.lock().unwrap()[2].embed.is_none(), "the abort is a line");
}

#[tokio::test]
async fn the_close_edits_the_prompt_in_place() {
    let data = data();
    let opened = game_with(&data, &["first", "second"]);
    data.gp_set_prompt_message(G, 0, TC, MessageId::new(77))
        .unwrap();
    let closed = data
        .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    let fake = Arc::new(FakeTransport::default());
    gp_after_close(playback(&data, &fake), closed).await.unwrap();
    // Nobody submitted, so the next round opens straight away.
    assert_eq!(fake.ops(), vec![Op::Edit(TC.get(), 77), Op::Send(TC.get())]);
}

#[tokio::test]
async fn a_close_whose_edit_fails_is_posted_instead() {
    let data = data();
    let opened = game_with(&data, &["first", "second"]);
    data.gp_set_prompt_message(G, 0, TC, MessageId::new(77))
        .unwrap();
    let closed = data
        .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    let fake = Arc::new(FakeTransport::default());
    *fake.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
    gp_after_close(playback(&data, &fake), closed).await.unwrap();
    assert_eq!(
        fake.ops(),
        vec![Op::Edit(TC.get(), 77), Op::Send(TC.get()), Op::Send(TC.get())]
    );
}

/// A game of `rounds` prompts, bob's one song submitted, the window closed,
/// and the song message recorded as message 88.
fn one_song_playing(data: &Data, rounds: &[&str]) {
    let opened = game_with_reveal(data, rounds, None, GpReveal::Round);
    submit(data, B, "bob", "Song");
    data.gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    data.gp_set_track_message(G, 0, 0, TC, MessageId::new(88))
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn the_reveal_replaces_the_song_message_and_its_controls() {
    let data = data();
    one_song_playing(&data, &["only"]);
    let fake = Arc::new(FakeTransport::default());
    gp_advance_track(playback(&data, &fake), 0, 0, false)
        .await
        .unwrap();
    assert_eq!(fake.ops()[0], Op::Edit(TC.get(), 88));
    let reveal = fake.sent.lock().unwrap()[0].clone();
    assert!(reveal.embed.is_some() && reveal.components.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_reveal_whose_edit_fails_is_posted_instead() {
    let data = data();
    one_song_playing(&data, &["only"]);
    let fake = Arc::new(FakeTransport::default());
    *fake.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
    gp_advance_track(playback(&data, &fake), 0, 0, false)
        .await
        .unwrap();
    assert_eq!(fake.ops()[..2], [Op::Edit(TC.get(), 88), Op::Send(TC.get())]);
}

#[tokio::test(start_paused = true)]
async fn the_rounds_last_reveal_posts_its_results_and_marks_them() {
    let data = data();
    one_song_playing(&data, &["first", "second"]);
    let fake = Arc::new(FakeTransport::default());
    gp_advance_track(playback(&data, &fake), 0, 0, false)
        .await
        .unwrap();
    // The reveal edit, the results, then round two's prompt.
    assert_eq!(
        fake.ops(),
        vec![Op::Edit(TC.get(), 88), Op::Send(TC.get()), Op::Send(TC.get())]
    );
    assert!(game(&data).rounds[0].results_posted);
}

#[tokio::test(start_paused = true)]
async fn a_failed_results_post_leaves_the_round_owed_and_the_game_moves_on() {
    let data = data();
    one_song_playing(&data, &["first", "second"]);
    let fake = Arc::new(FakeTransport::default());
    fail_sends(&fake, 1);
    gp_advance_track(playback(&data, &fake), 0, 0, false)
        .await
        .unwrap();
    assert!(!game(&data).rounds[0].results_posted, "the next resume posts it");
    assert_eq!(fake.ops().len(), 3, "round two still opens");
    assert_eq!(game(&data).current_round, 1);
}

#[tokio::test(start_paused = true)]
async fn the_games_last_reveal_posts_the_final_scoreboard() {
    let data = data();
    one_song_playing(&data, &["only"]);
    let fake = Arc::new(FakeTransport::default());
    gp_advance_track(playback(&data, &fake), 0, 0, false)
        .await
        .unwrap();
    let last = fake.ops().len() - 1;
    assert_eq!(embed_title(&fake, last), GP_GAME_OVER);
    assert!(!data.gp_is_active(G), "a finished game is removed");
}

#[tokio::test(start_paused = true)]
async fn the_window_warning_is_a_line() {
    let data = data();
    let opened = game_with(&data, &["first"]);
    submit(&data, B, "bob", "Song");
    let fake = Arc::new(FakeTransport::default());
    gp_spawn_window_timer_secs(playback(&data, &fake), opened.generation, TC, 40);
    tokio::time::sleep(Duration::from_secs(11)).await;
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "the warning, and the close not yet");
    assert!(sent[0].embed.is_none());
    assert!(sent[0].content.as_deref().unwrap().starts_with(GP_WINDOW_WARNING));
}

#[tokio::test]
async fn a_click_is_answered_once_privately_and_never_acknowledged() {
    let press = FakePress::default();
    gp_answer_component(&press, "noted".into()).await.unwrap();
    assert_eq!(
        press.ops(),
        vec![PressOp::Respond {
            ephemeral: true,
            text: "noted".into()
        }]
    );
}
```

Check, and adjust the test if they differ (don't change production code to fit a guess):
- the warning lead time is `GP_WARNING_SECS`; the test assumes 30, so with a 40 s timer the warning lands at 10 s;
- `GpGame` has a `current_round` field;
- `gp_close_window_if` with one submission and `GpReveal::Round` leaves the game `Playing`.

- [ ] **Step 2: Run them and watch them fail to compile**

Run: `cargo test -p crack-core --lib commands::music::gp::test_delivery`
Expected: compile errors: `GpPlayback` has no `transport` field, and there is no `gp_answer_component`.

- [ ] **Step 3: Implement** in `gp/playback.rs`.

`GpPlayback`:

```rust
/// What the playback side of a game needs: shared state, the wire to Discord,
/// the call, and the guild. Cloned into every per-track handler and timer task.
#[derive(Clone)]
pub struct GpPlayback {
    pub data: Arc<Data>,
    pub transport: Arc<dyn Transport>,
    pub call: Arc<Mutex<Call>>,
    pub guild_id: GuildId,
}
```

`gp_send` takes a card and retries over the courier:

```rust
/// Send a game message, retrying once: a rate limit or a transient 5xx should
/// not cost the guild its game. Both attempts failing is treated as fatal by the
/// callers, because everything the round needs is armed after the send.
async fn gp_send(
    pb: &GpPlayback,
    channel: GenericChannelId,
    card: GpCard,
) -> Result<MessageId, Error> {
    let out = gp_rendered(card);
    let first = match courier::post_message(&*pb.transport, channel, &out).await {
        Ok(id) => return Ok(id),
        Err(e) => e,
    };
    tracing::warn!(
        "gp: send in {} failed ({first}), retrying once",
        pb.guild_id
    );
    Ok(courier::post_message(&*pb.transport, channel, &out).await?)
}

/// Post a card to a channel. Best effort: the courier logs a failure.
pub async fn gp_post(
    data: &Data,
    transport: &dyn Transport,
    channel: GenericChannelId,
    card: GpCard,
) {
    courier::post(
        data,
        transport,
        Destination::Channel(channel),
        &card.into(),
        &RenderCx::now(),
    )
    .await;
}

/// Edit `card` into `message`, or post it if there is no message or the edit
/// fails (deleted by hand, or by `/clean`). Best effort.
async fn gp_edit_or_post(
    pb: &GpPlayback,
    message: Option<(GenericChannelId, MessageId)>,
    channel: GenericChannelId,
    card: GpCard,
) {
    let msg: CrackedMessage = card.into();
    if let Some((chan, id)) = message {
        match courier::edit_message(&*pb.transport, chan, id, &msg).await {
            Ok(()) => return,
            Err(e) => tracing::warn!("gp: editing {id} in {chan}: {e}; posting instead"),
        }
    }
    courier::post(
        &pb.data,
        &*pb.transport,
        Destination::Channel(channel),
        &msg,
        &RenderCx::now(),
    )
    .await;
}

/// Answer a dropdown pick or a 👍: one ephemeral response, worked out from
/// memory before this is called -- well inside Discord's three seconds, so
/// there is no acknowledge first (the spec, section 3).
pub(in crate::commands::music::gp) async fn gp_answer_component(
    press: &dyn Press,
    text: String,
) -> Result<(), Error> {
    courier::respond(press, &GpCard::Line(text).into(), &RenderCx::now(), true).await?;
    Ok(())
}
```

Then each site. Delete every `#[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]` in this file as you go; keep the three with other reasons:
- **`gp_abort`:** the final send becomes `gp_post(&pb.data, &*pb.transport, text_channel, GpCard::Line(GP_ABORTED.into())).await;`. Keep the comment above it.
- **`gp_open_round`:** `gp_send(pb, opened.text_channel, GpCard::Prompt(opened.clone()))`.
- **The window timer's warning:** `gp_post(&pb.data, &*pb.transport, text_channel, GpCard::Line(gp_warning_text(&warning))).await;`.
- **`gp_after_close`:** `gp_edit_or_post(&pb, closed.prompt_message, closed.text_channel, GpCard::PromptClosed(closed.clone())).await;`, then `gp_follow(pb, closed.next, closed.text_channel, false)` as before.
- **`gp_follow`'s `Finished`:** `courier::post_message(&*pb.transport, text_channel, &gp_rendered(GpCard::Scoreboard { scores, title: GP_GAME_OVER, lead: None })).await?;`.
- **`gp_play_track`:** `gp_send(pb, start.text_channel, GpCard::Song { start: start.clone(), guild: guild_id })`.
- **`gp_advance_track`:** `gp_edit_or_post(&pb, res.message, res.text_channel, GpCard::Reveal(res.clone())).await;`, then:

```rust
    if let Some(round) = &res.round {
        let out = gp_rendered(GpCard::RoundResults(round.clone()));
        match courier::post_message(&*pb.transport, res.text_channel, &out).await {
            Ok(_) => pb.data.gp_mark_results_posted(guild_id, round.round_idx),
            Err(e) => tracing::warn!(
                "gp: posting round {} results in {guild_id}: {e}",
                round.round_idx + 1
            ),
        }
    }
```

- **`handle_gp_component`:** keep everything up to `let content = ...` exactly as it is, then replace the `create_response` with:

```rust
    let press = DiscordPress {
        http: &ctx.http,
        interaction: mci,
    };
    gp_answer_component(&press, content).await
```

Imports: add `crate::messaging::{courier::{self, Destination}, message::CrackedMessage, render::RenderCx, transport::{DiscordPress, Press, Transport}}`. Remove the now-unused `CreateMessage`, `EditMessage`, `CreateInteractionResponse*`, `Http` and `CreateEmbed`. Let the compiler tell you which.

In `gp/commands.rs`'s `gp_playback`: `transport: Arc::new(DiscordTransport::of(ctx.serenity_context())),` in place of `http: ..`.
In `gp_persist.rs`'s `GpPlayback { .. }` literal: `transport: Arc::new(DiscordTransport::of(ctx)),` in place of `http: ctx.http.clone(),`.
Import `crate::messaging::transport::DiscordTransport` in both.

In `gp/mod.rs`, under `#[cfg(test)] mod test;` add:

```rust
#[cfg(test)]
mod test_delivery;
```

- [ ] **Step 4: Run the tests and the gates**

Run: `cargo test -p crack-core --lib commands::music::gp` → all pass.
Run: `grep -c 'messaging arc: not migrated yet' crack-core/src/commands/music/gp/playback.rs` → `0`.
Run the clippy and fmt gates → clean. An unfulfilled `#[expect]` left anywhere is a clippy error: delete it.

- [ ] **Step 5: Sabotage**
- `gp_send` returns the first error with no retry → `a_prompt_that_fails_once_is_sent_again` fails.
- `gp_edit_or_post` returns after a failed edit → both "posted instead" tests fail.
- `gp_mark_results_posted` is called on `Err` as well → the failed-results test fails.
- `gp_advance_track` posts the reveal with `gp_post` instead of `gp_edit_or_post` (no edit) → `the_reveal_replaces_the_song_message_and_its_controls` fails.
- `gp_answer_component` passes `ephemeral: false` → the click test fails.

Restore after each one.

- [ ] **Step 6: Commit**

```bash
git add crack-core/Cargo.toml Cargo.lock crack-core/src/commands/music/gp/playback.rs crack-core/src/commands/music/gp/commands.rs crack-core/src/commands/music/gp_persist.rs crack-core/src/commands/music/gp/mod.rs crack-core/src/commands/music/gp/test.rs crack-core/src/commands/music/gp/test_delivery.rs
git commit -m "gp: playback sends through the courier, and is tested against the fakes

GpPlayback carries a Transport instead of Http. Every send keeps its
failure behaviour: the prompt and song retry once and abort; the close and
reveal edit in place or post; results are marked only when posted. A
click is answered with one private response. test_delivery.rs covers
each of these against FakeTransport and FakePress.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

(`Cargo.lock` is listed in case the dev-dependency touches it; `git add` of an unchanged file is harmless.)

---

### Task 4: The commands on the courier

**Files:**
- Modify: `crack-core/src/commands/music/gp/commands.rs` (`gp`, `gp_pick_categories`, `gp_pick_update`, `gp_answer_vote`, `gp_status`, `gp_end`)

**Interfaces:**
- Consumes:
  - from Task 1: `courier::{reply_as, retire_reply, respond, update, post_message}`, `DiscordPress`, `DiscordTransport::of`;
  - from Task 2: `GpCard::{Rules, Status, Picker, Scoreboard, Line}`, `gp_rendered`.
- Produces: nothing new.

- [ ] **Step 1: Replace the picker's sends.** `gp_pick_categories`:

```rust
    let reply = courier::reply_as(
        ctx,
        GpCard::Picker {
            text: GP_PICK_TEXT.into(),
            picked: picked.clone(),
            open: true,
        }
        .into(),
        true,
    )
    .await?;
```

On timeout:

```rust
            courier::retire_reply(
                ctx,
                &reply,
                GpCard::Picker {
                    text: GP_PICK_TIMED_OUT.into(),
                    picked: vec![],
                    open: false,
                }
                .into(),
            )
            .await?;
            return Ok(None);
```

After a click is collected: `let press = DiscordPress { http: ctx.http(), interaction: &mci };`. A non-host click:

```rust
            courier::respond(
                &press,
                &GpCard::Line(GP_PICK_NOT_HOST.into()).into(),
                &RenderCx::now(),
                true,
            )
            .await?;
            continue;
```

Then `gp_pick_update` takes the press and the card's fields:

```rust
/// Answer a picker click by redrawing the picker.
#[cfg(not(tarpaulin_include))]
async fn gp_pick_update(
    press: &dyn Press,
    text: &str,
    picked: &[GpCategory],
    open: bool,
) -> Result<(), Error> {
    courier::update(
        press,
        &GpCard::Picker {
            text: text.to_string(),
            picked: picked.to_vec(),
            open,
        }
        .into(),
        &RenderCx::now(),
    )
    .await?;
    Ok(())
}
```

Its call sites become:
- Cancel: `gp_pick_update(&press, GP_PICK_CANCELLED, &[], false)`
- Start: `gp_pick_update(&press, &chosen, &[], false)`
- the redraw: `gp_pick_update(&press, GP_PICK_TEXT, &picked, true)`

This is the same output as before: `open: false` sends no rows, and `to_interaction_message` sets an empty list, which clears them.

- [ ] **Step 2: Replace the vote answers** in `gp_answer_vote`:

```rust
        let transport = DiscordTransport::of(ctx.serenity_context());
        let sent: Result<(), String> = match dm {
            Ok(dm) => courier::post_message(&transport, dm, &gp_rendered(GpCard::Line(mine.to_string())))
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
```

(Keep the `if let Err(e) = sent { tracing::warn!(..) }` that follows.) The room line:

```rust
    courier::post_message(
        &DiscordTransport::of(ctx.serenity_context()),
        channel,
        &gp_rendered(GpCard::Line(room.to_string())),
    )
    .await?;
```

- [ ] **Step 3: Replace the three embed replies**
- `gp`: `courier::reply_as(ctx, GpCard::Rules.into(), false).await?;`
- `gp_status`: `courier::reply_as(ctx, GpCard::Status(status).into(), false).await?;`
- `gp_end`: `courier::reply_as(ctx, GpCard::Scoreboard { scores: game.sorted_scores(), title: GP_SCOREBOARD, lead: None }.into(), false).await?;`

Delete the six "messaging arc: not migrated yet" expects. Remove the imports that are now unused (`CreateReply`, `CreateInteractionResponse*`, `CreateMessage`, `Cow`, and any `gp_*_embed` that is no longer called from here); add `crate::messaging::{courier, render::RenderCx, transport::{DiscordPress, DiscordTransport, Press}}`.

- [ ] **Step 4: Run the gates**

Run: `grep -c 'messaging arc: not migrated yet' crack-core/src/commands/music/gp/commands.rs` → `0`.
Run: `cargo test -p crack-core --lib` → all pass.
Run the clippy and fmt gates → clean.

These paths are covered by tests at the seam: the picker card's rows (Task 2), `respond`/`update`/`retire` (Task 1), and the click answer (Task 3). The collector loop itself is `cfg(not(tarpaulin_include))` and is covered by the TuneTitan checklist. Say so in your report rather than inventing a test that cannot see Discord.

- [ ] **Step 5: Sabotage**
- On timeout, `retire_reply` → `edit_reply`. Nothing catches this: the R4 drop is invisible to the unit tests. Report it as **uncaught (TuneTitan checklist item 1)**, and restore.
- `gp_pick_update`'s Cancel passes `open: true`. Also uncaught: report it the same way, and restore.

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/commands/music/gp/commands.rs
git commit -m "gp: the commands reply, pick and vote through the courier

The picker posts and redraws as a GpCard, answers a non-host privately,
and retires its buttons on timeout. Vote answers and the room line,
/gp, /gp status and /gp end's scoreboard go through the courier too.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 5: Resume on the courier

**Files:**
- Modify: `crack-core/src/commands/music/gp_persist.rs` (`abandon_resume`, `gp_resume_guild`, `post_owed_results`, `announce`; new `take_down_components`)
- Modify: `crack-core/src/commands/music/gp/test_delivery.rs` (resume tests)

**Interfaces:**
- Consumes:
  - from Task 3: `gp_post`, `GpPlayback.transport`;
  - from Task 2: `GpCard::{Scoreboard, RoundResults, Line}`, `gp_rendered`;
  - from Task 1: `Transport::clear_components`, `courier::post_message`.
- Produces:
  - `pub(crate) async fn post_owed_results(transport: &dyn Transport, game: &GpGame) -> Vec<usize>`;
  - `pub(crate) async fn take_down_components(transport: &dyn Transport, guild: GuildId, channel: GenericChannelId, id: MessageId)`.
  Both are `pub(crate)` so the tests in `gp/test_delivery.rs` can call them.

- [ ] **Step 1: Write the failing tests** at the end of `gp/test_delivery.rs`:

```rust
use crate::commands::music::gp_persist::{post_owed_results, take_down_components};

/// A two-round game whose first round has been revealed in memory but whose
/// results never reached the channel.
fn round_one_owed(data: &Data) {
    let opened = game_with_reveal(data, &["first", "second"], None, GpReveal::Round);
    submit(data, B, "bob", "Song");
    data.gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
}

#[tokio::test]
async fn owed_results_are_posted_and_reported() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    assert_eq!(post_owed_results(&fake, &game(&data)).await, vec![0]);
    assert_eq!(fake.ops(), vec![Op::Send(TC.get())]);
}

#[tokio::test]
async fn owed_results_that_fail_are_not_reported_posted() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    fail_sends(&fake, 1);
    assert!(post_owed_results(&fake, &game(&data)).await.is_empty());
}

#[tokio::test]
async fn taking_down_the_old_dropdown_clears_components_and_nothing_else() {
    let fake = FakeTransport::default();
    take_down_components(&fake, G, TC, MessageId::new(88)).await;
    assert_eq!(fake.ops(), vec![Op::ClearComponents(TC.get(), 88)]);
    assert!(fake.sent.lock().unwrap().is_empty(), "the embed is left alone");
}
```

If `GpGame::unposted_results()` needs anything more than `results_posted == false` and a non-empty `tracks` for round 0, set that up the way the existing `unposted_results` tests in `gp/test.rs` do.

- [ ] **Step 2: Run them and watch them fail to compile**

Run: `cargo test -p crack-core --lib commands::music::gp::test_delivery`
Expected: compile errors: `post_owed_results` is private and takes `&Http`, and there is no `take_down_components`.

- [ ] **Step 3: Implement** in `gp_persist.rs`:

```rust
/// Post the results of every round the game moved past without them reaching
/// the channel (see [`GpGame::unposted_results`]). Returns the rounds posted;
/// marking them is the caller's, since only a game back in the map can be
/// written down.
pub(crate) async fn post_owed_results(transport: &dyn Transport, game: &GpGame) -> Vec<usize> {
    let mut posted = Vec::new();
    for idx in game.unposted_results() {
        let out = gp_rendered(GpCard::RoundResults(game.round_result(idx)));
        match courier::post_message(transport, game.text_channel, &out).await {
            Ok(_) => posted.push(idx),
            Err(e) => tracing::warn!(
                "gp: posting round {} results in {} after a restart: {e}",
                idx + 1,
                game.guild_id
            ),
        }
    }
    posted
}

/// Take the dropdown off a song message from before a restart, leaving its
/// embed: the reveal only edits the message the track remembers, which is
/// about to be a new one. Best effort.
pub(crate) async fn take_down_components(
    transport: &dyn Transport,
    guild: GuildId,
    channel: GenericChannelId,
    id: MessageId,
) {
    if let Err(e) = transport.clear_components(channel, id).await {
        tracing::warn!("gp: taking down the pre-restart song message in {guild}: {e}");
    }
}
```

In `gp_resume_guild`, build the transport once, near the top after the early returns: `let transport: Arc<dyn Transport> = Arc::new(DiscordTransport::of(ctx));`. Then:
- the `GpPlayback` literal uses `transport: transport.clone()`;
- `post_owed_results(&*transport, &game)`;
- `abandon_resume(.., &*transport)`: change its last parameter to `transport: &dyn Transport` and its send to `gp_post(data, transport, text_channel, GpCard::Line(GP_LOST.into())).await;`;
- the finished game's scoreboard: `gp_post(data, &*transport, text_channel, GpCard::Scoreboard { scores, title: GP_GAME_OVER, lead: None }).await;`;
- the lost game's scoreboard: `gp_post(data, &*transport, text_channel, GpCard::Scoreboard { scores, title: GP_SCOREBOARD, lead: Some(GP_LOST) }).await;`;
- the pre-restart dropdown: `take_down_components(&*pb.transport, guild_id, c, m).await;`;
- `announce`: `gp_post(&pb.data, &*pb.transport, text_channel, GpCard::Line(format!("{GP_RESUMED} {what}"))).await;`.

The courier's own failure log replaces the per-site `tracing::warn!`s for the best-effort posts. That's expected; don't re-add them.

Delete the six "messaging arc: not migrated yet" expects and the unused `Http`, `CreateMessage`, `EditMessage`, `CreateComponent` and `gp_*_embed` imports.

- [ ] **Step 4: Run the tests and the gates**

Run: `grep -rc 'messaging arc: not migrated yet' crack-core/src/commands/music/gp crack-core/src/commands/music/gp_persist.rs` → every count `0`.
Run: `cargo test -p crack-core --lib` → all pass.
Run the clippy and fmt gates → clean.

- [ ] **Step 5: Sabotage**
- `post_owed_results` pushes `idx` on `Err` too → `owed_results_that_fail_are_not_reported_posted` fails.
- `take_down_components` calls `transport.edit(channel, id, Rendered::default())` → the take-down test fails (an `Edit` op and a recorded render).

Restore after each one.

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/commands/music/gp_persist.rs crack-core/src/commands/music/gp/test_delivery.rs
git commit -m "gp: resume posts through the courier, and keeps the old song's embed

Owed results, scoreboards and the resumed/lost lines go through the
courier. Taking the dropdown off a pre-restart song message uses
Transport::clear_components, which leaves the embed alone where an edit
would have wiped it. /gp has no messaging expects left.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 6: Release prep (v0.24.1)

**Files:**
- Modify: `Cargo.toml` (`[workspace.package] version = "0.24.1"`)
- Modify: `Cargo.lock` (whatever `cargo build` writes for the version bump)
- Modify: `CHANGELOG.md` (under `## Unreleased`)

- [ ] **Step 1: Bump the version.** In the root `Cargo.toml`, set `[workspace.package] version` from `"0.24.0"` to `"0.24.1"`. Run `cargo build -p crack-core` so `Cargo.lock` updates the members' versions.

- [ ] **Step 2: Write the CHANGELOG entries** under `## Unreleased`. Add a `### Fixed` bullet:

```markdown
- **`/gp` song titles with `*`, `_`, backticks or `@` broke the song, reveal
  and results embeds' formatting.** They are now escaped like every other
  title since v0.22.0, and a pathological title is capped at 100 characters
  (YouTube's own limit, so a real title is never cut). Player names in
  `/gp status` are escaped too.
```

and a `### Changed` bullet:

```markdown
- **`/gp` sends everything through the messaging layer.** Its prompts, songs,
  reveals, results, scoreboards, picker and button answers are rendered by
  the one renderer and delivered by the courier, like the music commands
  since v0.22.0, and clippy now refuses a raw send anywhere in `/gp`. Nothing
  else about the game changes.
```

If `### Fixed` and `### Changed` already exist under `## Unreleased`, add to them rather than creating second ones.

- [ ] **Step 3: The full gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
cargo test --workspace
grep -rn 'messaging arc: not migrated yet' crack-core/src/commands/music/gp crack-core/src/commands/music/gp_persist.rs
```

Expected: fmt and clippy clean; tests pass; the grep prints nothing. The db-tests run needs the local postgres described in the standing rules. The controller runs it before the PR; you don't need to.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "v0.24.1: /gp on the courier

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```
