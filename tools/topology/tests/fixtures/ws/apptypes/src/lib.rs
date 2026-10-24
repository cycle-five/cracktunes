pub use fakecord::GuildId;

pub type SharedCtx = std::sync::Arc<fakecord::Context>;

pub struct Plain {
    pub n: u32,
}
