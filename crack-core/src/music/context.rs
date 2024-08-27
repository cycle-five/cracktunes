use crate::commands::{play_utils::QueryType, Mode};
use crate::Context as CrackContext;
use poise::serenity_prelude as serenity;
use serenity::all::Message;
use songbird::Call;

use std::sync::{Arc, Mutex};

/// QueryContext is the context for a query to the bot for a song.
pub struct QueryContext<'a> {
    // The poise context for the bot.
    ctx: CrackContext<'a>,
    // The call the bot is in.
    call: Arc<Mutex<Call>>,
    // The playmode used for the query.
    mode: Mode,
    // The query type, perhaps a misnomer?
    query_type: QueryType,
    // The search message we update with context and results.
    search_msg: &'a mut Message,
}

/// Implementation of `[QueryContext]``.
impl QueryContext<'_> {
    /// Create a new QueryContext.
    pub fn new<'a>(
        ctx: CrackContext<'a>,
        call: Arc<Mutex<Call>>,
        mode: Mode,
        query_type: QueryType,
        search_msg: &'a mut Message,
    ) -> QueryContext<'a> {
        QueryContext {
            ctx,
            call,
            mode,
            query_type,
            search_msg,
        }
    }

    /// Get the poise context.
    pub fn ctx(&self) -> &CrackContext {
        &self.ctx
    }

    /// Get the call.
    pub fn call(&self) -> Arc<Mutex<Call>> {
        self.call.clone()
    }

    /// Get the playmode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Get the query type.
    pub fn query_type(&self) -> &QueryType {
        &self.query_type
    }

    /// Get the search message.
    pub fn search_msg(&mut self) -> &mut Message {
        self.search_msg
    }
}

mod tests {
    #[test]
    fn test_query_context() {
        // TODO
    }
}
