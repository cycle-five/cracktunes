pub struct Track {
    pub id: u32,
}

pub fn pick_next(queue: &[Track]) -> Option<&Track> {
    queue.first()
}

pub fn score(t: &Track) -> u32 {
    t.id * 2
}

pub fn in_guild(g: &apptypes::GuildId) -> bool {
    g.0 > 0
}

pub fn uses_alias(c: apptypes::SharedCtx) -> usize {
    std::sync::Arc::strong_count(&c)
}

pub fn greet() -> String {
    let c = fakecord::Context;
    c.say("hi");
    String::new()
}

pub fn wrapper() -> String {
    greet()
}

pub fn plain(p: apptypes::Plain) -> u32 {
    p.n
}
