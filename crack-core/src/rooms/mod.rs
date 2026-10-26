pub mod authority;
pub mod card;
pub mod discord;
pub mod facts;
pub mod guard;
pub mod ids;
pub mod interact;
pub mod lifecycle;
pub mod ops;
pub mod overwrites;
pub mod preflight;
pub mod registry;
pub mod settings;
pub mod state;
#[cfg(test)]
pub mod test_support;

/// The three fields of `Data` that the voice rooms and the mod log read, as
/// a handle that does not name `Data`. Same names, so a body reads the same;
/// the `Arc`s keep the registries the same ones the commands read.
#[derive(Debug, Clone, Default)]
pub struct RoomsData {
    pub database_pool: Option<sqlx::PgPool>,
    pub rooms: std::sync::Arc<registry::RoomRegistry>,
    pub modlog: std::sync::Arc<crate::modlog::registry::ModLogRegistry>,
}
