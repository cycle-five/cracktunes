//! The poise half of the ops layer's types: how a slash or prefix command
//! becomes an [`Actor`], an [`OpCx`], and how it replies before it settles.
//! Extension traits, not inherent methods, so the types can move to another
//! crate without poise following them.

use super::{audit::Actor, ops::Done, ops::OpCx};
use crate::{
    errors::CrackedError,
    messaging::{courier, message::CrackedMessage},
    Context,
};

/// An [`Actor`] from the command being run.
pub trait ActorFromCtx {
    /// The member running this command, in the channel they ran it from.
    #[must_use]
    fn from_ctx(ctx: &Context<'_>) -> Actor;
}

impl ActorFromCtx for Actor {
    fn from_ctx(ctx: &Context<'_>) -> Actor {
        let is_prefix = matches!(ctx, poise::Context::Prefix(_));
        Self::for_command(
            ctx.author().id,
            is_prefix,
            ctx.command().qualified_name.clone(),
            Some(ctx.channel_id()),
        )
    }
}

/// An [`OpCx`] from the command being run.
pub trait OpCxFromCtx {
    /// The member running this command.
    fn from_ctx(ctx: &Context<'_>) -> Result<OpCx, CrackedError>;
}

impl OpCxFromCtx for OpCx {
    fn from_ctx(ctx: &Context<'_>) -> Result<OpCx, CrackedError> {
        let sc = ctx.serenity_context();
        Ok(OpCx {
            data: ctx.data().music_data(),
            http: sc.http.clone(),
            cache: sc.cache.clone(),
            guild_id: ctx.guild_id().ok_or(CrackedError::NoGuildId)?,
            actor: Actor::from_ctx(ctx),
        })
    }
}

/// A [`Done`] that replies through the command's context, then settles.
pub trait ReplyThenSettle<T> {
    /// Reply to the command with `msg`, then settle below that reply, so a
    /// re-rendered status lands under the command's answer, as `/skip`'s
    /// does. A failed reply still settles (with no floor) before its error is
    /// returned: the op happened either way.
    fn reply_then_settle(
        self,
        ctx: Context<'_>,
        cx: &OpCx,
        msg: CrackedMessage,
    ) -> impl std::future::Future<Output = Result<T, CrackedError>>;
}

impl<T> ReplyThenSettle<T> for Done<T> {
    async fn reply_then_settle(
        self,
        ctx: Context<'_>,
        cx: &OpCx,
        msg: CrackedMessage,
    ) -> Result<T, CrackedError> {
        let reply = courier::reply(ctx, msg).await;
        let anchor = match &reply {
            Ok(handle) => courier::locate(ctx, handle).await,
            Err(_) => None,
        };
        let outcome = self.settle_after(cx, anchor).await;
        reply.map(|_| outcome)
    }
}
