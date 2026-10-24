pub struct Handler;

impl fakecord::EventHandler for Handler {
    fn on_event(&self, ctx: &fakecord::Context) {
        let _ = ctx;
    }
}

pub struct Bot;

impl Bot {
    pub fn ping(&self) -> u32 {
        1
    }
}

#[fakecord::command]
pub fn cmd() -> u32 {
    crate::rules::score(&crate::rules::Track { id: 1 })
}

pub fn third(w: &crate::state::Wide) -> u32 {
    w.f3
}
