pub use fakechars;
pub use fakecord_macros::command;

pub struct GuildId(pub u64);

pub struct Context;

impl Context {
    pub fn say(&self, _s: &str) {}
}

pub trait EventHandler {
    fn on_event(&self, ctx: &Context);
}
