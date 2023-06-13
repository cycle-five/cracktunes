pub mod client;
pub mod commands;
pub mod connection;
pub mod errors;
pub mod guild;
pub mod handlers;
pub mod messaging;
pub mod sources;
pub mod utils;

#[cfg(test)]
pub mod test;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;
// User data, which is stored and accessible in all command invocations
pub struct Data {}
