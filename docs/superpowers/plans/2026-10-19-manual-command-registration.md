# Manual Command Registration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `COMMAND_REGISTRATION=manual` setting that makes TuneTitan register only `/register` globally, so servers opt in to its commands, while CrackTunes (unset) behaves exactly as today.

**Architecture:** A `CommandRegistration` mode on `BotConfig`, read from the environment. The mode decides three things: the `/register` command's permissions, what startup registers globally (plus a guild refresh in manual mode), and which `/register` buttons a caller gets. Startup talks to Discord through a small trait seam (`CommandsDiscord`), so tests record exactly what would be sent, the same way `rooms::discord::RoomsDiscord` does.

**Tech Stack:** Rust, poise (serenity-next branch), serenity `next`, serde, `serenity::async_trait`.

**Spec:** The design was approved in chat on 2026-10-19. It is reproduced here as §Design and is the whole spec.

## Design (approved 2026-10-19)

- **The setting:** `COMMAND_REGISTRATION` is `global` (the default when unset) or `manual`. TuneTitan sets `manual` in homelab `tunetitan/docker-compose.yml`. CrackTunes production does not set it and must behave exactly as v0.29.0.
- **Startup, manual mode:** register exactly ONE global command, `/register`. This replaces the full global set, so a server where TuneTitan was never registered shows only that one command, and only to Manage Server members.
- **Startup refresh, manual mode (owner approved):** at each startup, every server that already has TuneTitan's server-level commands gets them rewritten to the current build's set. Servers with none are left untouched.
- **`/register` in manual mode:** requires Manage Server (`required_permissions` AND `default_member_permissions`), so only mods and up see or use it.
  - **Buttons:** non-owners get "Register in guild" and "Delete in guild". The two global buttons stay bot-owner-only.
  - **"Register in guild"** writes every command EXCEPT `/register`, because the global copy already covers it (a second copy would show twice).
- **`/register` in global mode (CrackTunes):** unchanged. It needs Administrator, is for bot owners only, offers all four buttons, and "Register in guild" writes every command.
- **Version:** v0.30.0 (minor). TuneTitan only. Production stays on v0.29.0 for its soak.

## Global Constraints

- Keep the startup log line's exact phrase `Registered {n} global commands`. homelab `tunetitan/verify.sh` and `bots/verify.sh` grep for `Registered `…` global commands`.
- Global mode must send byte-for-byte what v0.29.0 sends: same commands, same `/register` permissions, same buttons.
- Model data with typed structs/enums and serde. No `serde_json::json!` and no `serde_json::Value`, tests included (owner's global rule).
- New user-facing strings become `pub const`s in `crack-core/src/messaging/messages.rs`. Log lines stay inline.
- Unit tests go in an in-file `#[cfg(test)] mod test`, matching the crate.
- Sabotage every new test: break the code it guards, watch it fail, restore. Report each mutation → test in your task report.
- Never `git add -A`. Add files by path.
- `cargo fmt`, plus `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Review Focus

1. **A blank `COMMAND_REGISTRATION=`** (compose's `${VAR:-}` pattern) must mean unset → global, not an error. Pinned in Task 1 (`blank_env_means_unset`).
2. **A typo** (`manaul`) must stop the bot at startup with a message naming the variable and both accepted values. Silently falling back to global would flood the shared server with all 48 commands again. Pinned in Task 1 (`unknown_value_is_an_error`).
3. **A non-owner pressing a global button that wasn't offered** (a crafted interaction) is refused. Pinned in Task 4 (`only_offered_buttons_may_be_pressed`).
4. **One server's command lookup failing at startup** (an outage, or the bot was kicked) must not stop the refresh of the others. Pinned in Task 3 (`refresh_continues_past_a_failing_guild`).
5. **The refresh writing `/register` into a server** would show it twice there. Pinned in Task 3 (`refresh_writes_guild_set_only_where_commands_exist` asserts the exact names sent).

---

### Task 1: The registration mode and its configuration

**Files:**
- Create: `crack-core/src/commands/registration.rs`
- Modify: `crack-core/src/commands/mod.rs` (add `pub mod registration;`)
- Modify: `crack-core/src/lib.rs` (`BotConfig` at ~206: add the field; update `Default` ~223 and `Display` ~243)
- Modify: `crack-cli/src/main.rs` (`load_bot_config` ~149: env override after the `DATABASE_URL` block)
- Modify: `README.md` (paragraph after the optional-variables table, before "Autoplay (`/autoplay`)…"); `.env.example` (after the `SLEEVENOTE_BASE_URL` block)

**Interfaces:**
- Produces, in `crate::commands::registration`:
  - `pub const COMMAND_REGISTRATION_ENV: &str = "COMMAND_REGISTRATION";`
  - `#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)] #[serde(rename_all = "snake_case")] pub enum CommandRegistration { #[default] Global, Manual }`
  - `impl FromStr for CommandRegistration { type Err = UnknownCommandRegistration; }`. It trims, compares ASCII case-insensitively, and accepts only `global` / `manual`. Blank is an error at this level.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct UnknownCommandRegistration(pub String);` implementing `Display` and `std::error::Error`.
  - `impl CommandRegistration { pub fn from_env_value(raw: Option<&str>) -> Result<Option<Self>, UnknownCommandRegistration>; pub fn from_env() -> Result<Option<Self>, UnknownCommandRegistration>; }`. `None` or a blank value gives `Ok(None)`; anything else is parsed. `from_env` calls `from_env_value(std::env::var(COMMAND_REGISTRATION_ENV).ok().as_deref())`.
  - `BotConfig.command_registration: CommandRegistration` with `#[serde(default)]`.

- [ ] **Step 1: Write the failing tests** in `registration.rs`:
  - `parses_both_modes`: `"global"`→`Global`, `"manual"`→`Manual`, `" Manual\n"`→`Manual`, `"MANUAL"`→`Manual`.
  - `unknown_value_is_an_error`: `"manaul".parse::<CommandRegistration>()` is `Err`. Its `to_string()` contains `COMMAND_REGISTRATION`, `manaul`, `global` and `manual`.
  - `blank_env_means_unset`: `from_env_value(None)`, `from_env_value(Some(""))` and `from_env_value(Some("  "))` are all `Ok(None)`; `from_env_value(Some("manual"))` is `Ok(Some(Manual))`; `from_env_value(Some("x"))` is `Err`.
  - `config_defaults_to_global`: `BotConfig::default().command_registration == Global`; `serde_json::from_str::<BotConfig>("{}")` gives `Global`; `serde_json::from_str::<BotConfig>(r#"{"command_registration":"manual"}"#)` gives `Manual`.
- [ ] **Step 2:** `cargo test -p crack-core registration` fails to compile (the type doesn't exist).
- [ ] **Step 3: Implement** the Interfaces above. The error's `Display` is exactly: `COMMAND_REGISTRATION must be \`global\` or \`manual\`, not \`{value}\``. Add `command_registration: {:?}` to `BotConfig`'s `Display`.
- [ ] **Step 4: Wire the env override** in `load_bot_config`, after the `DATABASE_URL` block: `if let Some(mode) = CommandRegistration::from_env()? { config.command_registration = mode; }`. A bad value stops startup, the same way a missing `DISCORD_TOKEN` does.
- [ ] **Step 5: Docs.** README, as a paragraph after the table:

  > `COMMAND_REGISTRATION` decides how the slash commands reach Discord. `global`, the default, registers every command globally at startup. `manual` registers only `/register` globally, for members with Manage Server: a server gets the rest when one of them runs `/register` and presses *Register in guild*, and keeps them (refreshed at each startup) until *Delete in guild*. TuneTitan, the beta bot, runs `manual`, so it only shows up in the servers testing it.

  `.env.example`:
  ```
  # How slash commands reach Discord: `global` (the default) registers them all
  # at startup; `manual` registers only /register, and each server opts in with it.
  #COMMAND_REGISTRATION=global
  ```
- [ ] **Step 6:** `cargo test -p crack-core registration` passes. Sabotage: make `from_env_value` parse blank (expect `blank_env_means_unset` to fail), and drop `#[serde(default)]` (expect `config_defaults_to_global` to fail). Restore both.
- [ ] **Step 7: Commit** the touched files by path: `feat: COMMAND_REGISTRATION setting (global | manual)`.

### Task 2: What each mode registers

**Files:**
- Modify: `crack-core/src/commands/registration.rs`
- Modify: `crack-core/src/commands/register.rs` (`create_application_commands_cracked` ~25: share its builder with a filtered variant)
- Modify: `crack-core/src/commands/mod.rs` (add `commands_for`, near `commands_to_register` ~83)

**Interfaces:**
- Consumes: `CommandRegistration` (Task 1).
- Produces:
  - `pub const REGISTER_COMMAND_NAME: &str = "register";` in `registration.rs`.
  - `pub fn commands_for(mode: CommandRegistration) -> Vec<crate::Command>` in `commands/mod.rs`. It returns `all_commands()`. In `Manual` mode, the command named `REGISTER_COMMAND_NAME` gets `required_permissions = Permissions::MANAGE_GUILD` and `default_member_permissions = Permissions::MANAGE_GUILD` (replacing, not adding). `Global` returns `all_commands()` untouched.
  - `impl CommandRegistration { pub fn global_commands<U, E>(self, commands: &[poise::Command<U, E>]) -> Vec<serenity::CreateCommand<'static>>; pub fn guild_commands<U, E>(self, commands: &[poise::Command<U, E>]) -> Vec<serenity::CreateCommand<'static>>; }`
    - `Global`: both return everything, exactly what `create_application_commands_cracked(commands)` returns.
    - `Manual`: `global_commands` returns only `register`; `guild_commands` returns everything except `register`.
  - Filtering is on top-level `poise::Command::name` BEFORE building. Refactor `create_application_commands_cracked` into a private builder that takes a `keep: impl Fn(&poise::Command<U, E>) -> bool`. The public function keeps its signature and passes `|_| true`.
  - Test helper (in the test module, reused by Tasks 3 and 4 via `pub(crate)` under `#[cfg(test)]`): `fn sent_names(cmds: &[serenity::CreateCommand<'_>]) -> Vec<String>`. It serializes each command with `serde_json::to_string` and deserializes into `#[derive(Deserialize)] struct SentCommand { name: String }`. No `Value`.

- [ ] **Step 1: Write the failing tests** in `registration.rs` (`use crate::commands::commands_for`):
  - `register_is_for_mods_in_manual_mode`: in `commands_for(Manual)`, `register` has `required_permissions == MANAGE_GUILD` and `default_member_permissions == MANAGE_GUILD`.
  - `register_is_unchanged_in_global_mode`: in `commands_for(Global)`, `register` has `required_permissions == ADMINISTRATOR` and `default_member_permissions == Permissions::empty()`.
  - `both_modes_have_the_same_commands`: the names of `commands_for(Manual)` == the names of `commands_for(Global)` == the names of `all_commands()`.
  - `register_constant_names_the_command`: `crate::commands::register().name == REGISTER_COMMAND_NAME`.
  - `manual_registers_only_register_globally`: `sent_names(&Manual.global_commands(&commands_for(Manual))) == ["register"]`.
  - `manual_guild_set_is_everything_but_register`: `sent_names(&Manual.guild_commands(&cmds))` == `sent_names(&create_application_commands_cracked(&cmds))` with `"register"` removed, where `cmds = commands_for(Manual)`.
  - `global_mode_sends_what_it_always_did`: for `cmds = commands_for(Global)`, both `Global.global_commands(&cmds)` and `Global.guild_commands(&cmds)` serialize (`serde_json::to_string` of the whole Vec) byte-equal to `create_application_commands_cracked(&cmds)`.
- [ ] **Step 2:** `cargo test -p crack-core registration` fails (the functions don't exist).
- [ ] **Step 3: Implement** the Interfaces.
- [ ] **Step 4:** The tests pass. Sabotage: flip `guild_commands`' filter to keep `register`; skip the `MANAGE_GUILD` override; make `Manual.global_commands` return everything. Each must fail a test. Restore.
- [ ] **Step 5: Commit:** `feat: the command sets each registration mode sends`.

### Task 3: Startup sync and the guild refresh

**Files:**
- Modify: `crack-core/src/commands/registration.rs` (trait, live impl, `sync_global`, `refresh_guilds`, a fake in the test module)
- Modify: `crack-core/src/handlers/serenity.rs` (`on_ready` ~141–167)
- Modify: `crack-core/src/config.rs:119` (`all_commands()` → `commands_for(config.command_registration)`)

**Interfaces:**
- Consumes: `CommandRegistration::{global_commands, guild_commands}`, `commands_for` (Task 2).
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct CommandsError(pub String); // + Display, From<serenity::Error>

  #[serenity::async_trait]
  pub trait CommandsDiscord: Send + Sync {
      async fn set_global(&self, commands: &[CreateCommand<'static>]) -> Result<(), CommandsError>;
      async fn guild_command_count(&self, guild: GuildId) -> Result<usize, CommandsError>;
      async fn set_guild(&self, guild: GuildId, commands: &[CreateCommand<'static>]) -> Result<(), CommandsError>;
  }
  pub struct LiveCommandsDiscord { pub http: Arc<Http> } // Command::set_global_commands / GuildId::get_commands(..).len() / GuildId::set_commands

  #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
  pub struct GuildRefresh { pub checked: usize, pub refreshed: usize, pub failed: usize }

  /// Puts this mode's global set on Discord; Ok(how many commands that was).
  pub async fn sync_global<U, E>(api: &dyn CommandsDiscord, mode: CommandRegistration, commands: &[poise::Command<U, E>]) -> Result<usize, CommandsError>;
  /// Manual mode: for each guild, count its commands; where > 0, set_guild(guild_commands). A failed count or set is
  /// logged at WARN with the guild id, counted in `failed`, and the loop continues. Global mode: no calls, all zeros.
  pub async fn refresh_guilds<U, E>(api: &dyn CommandsDiscord, mode: CommandRegistration, commands: &[poise::Command<U, E>], guilds: &[GuildId]) -> GuildRefresh;
  ```
  The test fake `FakeCommandsDiscord` records `enum Call { SetGlobal(Vec<String>), GuildCount(GuildId), SetGuild(GuildId, Vec<String>) }` (names via `sent_names`). It is configurable per guild with an existing count and a failure for the count or the set.

- [ ] **Step 1: Write the failing tests** (`#[tokio::test]`):
  - `global_mode_syncs_everything_and_refreshes_nothing`: `sync_global(Global)` returns `Ok(n)` with `n == create_application_commands_cracked(&commands).len()` (all registered). Calls are exactly `[SetGlobal(all names)]`. `refresh_guilds(Global, …, &[g1, g2])` makes no calls and returns `GuildRefresh::default()`.
  - `manual_mode_syncs_only_register`: `sync_global(Manual)` returns `Ok(1)`; calls are exactly `[SetGlobal(["register"])]`.
  - `refresh_writes_guild_set_only_where_commands_exist`: g1 has 0 and g2 has 3. Calls are exactly `[GuildCount(g1), GuildCount(g2), SetGuild(g2, all names minus "register")]`, and the result is `{checked: 2, refreshed: 1, failed: 0}`.
  - `refresh_continues_past_a_failing_guild`: g1's count fails, g2 has 3, g3 has 2 but its set fails. All three are counted, g2 and g3 each get one `SetGuild`, and the result is `{checked: 3, refreshed: 1, failed: 2}`.
  - `sync_global_reports_failure`: the fake fails `set_global` → `Err`.
- [ ] **Step 2:** The tests fail to compile.
- [ ] **Step 3: Implement** the Interfaces in `registration.rs`.
- [ ] **Step 4: Wire `on_ready`.**
  - `let mode = self.data.bot_settings.command_registration; let commands = crate::commands::commands_for(mode);`
  - Inside the existing once-per-process gate, call `sync_global` with a `LiveCommandsDiscord { http: ctx.http.clone() }`.
    - On `Ok(n)`: log `Registered {n} global commands` exactly as today. In `Manual` mode, also log at INFO: `Command registration is manual: only /register is global, and servers opt in with it`.
    - On `Err`: keep today's error log and gate reset.
  - OUTSIDE the gate, on every `Ready`, when `mode == Manual`: collect `ready.guilds` ids and `tokio::spawn` `refresh_guilds`, then log at INFO `Refreshed commands in {refreshed} of {checked} servers ({failed} failed)`. Each `Ready` covers its own shard's guilds; a repeat after a reconnect rewrites the same set, which is harmless.
  - Update the comment above the gate to say the mode decides the set.
- [ ] **Step 5: Use the same commands in the framework.** In `config.rs:119`, use `commands_for(config.command_registration)`. The framework's `/register` must carry the same permissions as the registered one, or poise checks Administrator while Discord shows it to mods.
- [ ] **Step 6:** The tests pass and `cargo build -p cracktunes` succeeds. Sabotage: make `refresh_guilds` skip the count check (set everywhere); make a failed count `return` instead of `continue`; make `Global` refresh call anything. Each must fail a test. Restore.
- [ ] **Step 7: Commit:** `feat: manual mode registers /register globally and refreshes opted-in servers at startup`.

### Task 4: `/register` for mods in manual mode

**Files:**
- Modify: `crack-core/src/commands/register.rs` (`register_application_commands_buttons_cracked` ~96)
- Modify: `crack-core/src/commands/mod.rs` (`register` ~146: pass the mode)
- Modify: `crack-core/src/messaging/messages.rs` (a new const)

**Interfaces:**
- Consumes: `CommandRegistration`, `guild_commands` (Task 2).
- Produces, in `register.rs`:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum RegisterButton { RegisterGuild, UnregisterGuild, RegisterGlobal, UnregisterGlobal }`, with `fn custom_id(self) -> &'static str` (the existing ids: `register.guild`, `unregister.guild`, `register.global`, `unregister.global`), `fn from_custom_id(id: &str) -> Option<Self>`, and `fn button(self) -> serenity::CreateButton<'static>` (today's labels, styles and emoji).
  - `pub fn offered_buttons(mode: CommandRegistration, is_owner: bool) -> Vec<RegisterButton>`. An owner gets all four, in today's order, in either mode. A non-owner gets `[RegisterGuild, UnregisterGuild]` in `Manual` and `[]` in `Global`.
  - `register_application_commands_buttons_cracked<U: Sync + Send + 'static, E>(ctx: poise::Context<'_, U, E>, mode: CommandRegistration)`.
  - `messages::REGISTER_OWNER_ONLY: &str = "Can only be used by bot owner"` (today's text, moved).

- [ ] **Step 1: Write the failing tests** (in `register.rs`'s test module; create one if it has none):
  - `offered_buttons_by_mode_and_owner`: all four `(mode, is_owner)` cases, with exact `Vec`s as specified above.
  - `custom_ids_round_trip`: for every variant, `from_custom_id(b.custom_id()) == Some(b)`, and `from_custom_id("nope") == None`.
  - `only_offered_buttons_may_be_pressed`: `!offered_buttons(Manual, false).contains(&RegisterGlobal)` and `!…contains(&UnregisterGlobal)`. This is the predicate the handler uses to refuse.
- [ ] **Step 2:** The tests fail to compile.
- [ ] **Step 3: Implement.**
  - `let offered = offered_buttons(mode, is_owner)`. If it's empty, say `REGISTER_OWNER_ONLY` and return; that's today's behaviour for non-owners in global mode.
  - Send only `offered` as buttons.
  - Map the pressed id with `from_custom_id`. An unknown id is a WARN and return, as today. A press that isn't in `offered` gets `REGISTER_OWNER_ONLY` and returns.
  - `RegisterGuild` writes `mode.guild_commands(&ctx.framework().options().commands)`, and its "Registering {n} guild commands" counts that set.
  - The global branches are unchanged.
  - In `commands/mod.rs`, `register` passes `ctx.data().bot_settings.command_registration`.
- [ ] **Step 4:** The tests pass. Sabotage: offer the global buttons to non-owners in `Manual`; give non-owners the guild buttons in `Global`. Each must fail a test. Restore.
- [ ] **Step 5: Commit:** `feat: /register lets mods register this server in manual mode`.

---

## After the tasks (controller)

1. **Release prep:**
   - Bump `[workspace.package] version` in the root `Cargo.toml` from `0.29.0` to `0.30.0`, and refresh `Cargo.lock` with `cargo check`.
   - Run the full `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`.
2. **PR:** body carries the design, the mutation → caught-by table, and the TuneTitan hand-test checklist. Then CI, then review-bot triage.
3. **TuneTitan, before the merge:**
   - Dispatch `docker.yml` on the branch, then pin both TuneTitan images to `:feat-manual-register`, pulling them explicitly.
   - homelab branch: `COMMAND_REGISTRATION: manual` in `tunetitan/docker-compose.yml`.
   - `up` + `verify`. The log must show the manual line. Discord's global command list for TuneTitan must be exactly `[register]`.
4. **Ship:** merge, tag `v0.30.0` (signed), and let cargo-dist's Release create the GitHub release. Repin TuneTitan to `v0.30.0`, verify, then merge the homelab PR.
