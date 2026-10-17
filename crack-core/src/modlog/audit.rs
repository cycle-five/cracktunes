//! Where the mod log reads Discord's audit log: one recent page of one
//! action type. Tests script it with `test_support::FakeAudit`.
use super::attribution::{AuditKind, AuditRow};
use serenity::all::{AuditLogEntry, ChannelId, GuildId, Http, UserId};
use serenity::async_trait;
use serenity::http::{HttpError, JsonErrorCode, StatusCode};
use serenity::model::guild::audit_log::{Action, MemberAction};
use serenity::nonmax::NonMaxU8;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditError {
    /// 403, or Discord code 50013: the bot lacks View Audit Log.
    Forbidden,
    Other(String),
}

impl From<serenity::Error> for AuditError {
    fn from(err: serenity::Error) -> Self {
        if let serenity::Error::Http(HttpError::UnsuccessfulRequest(response)) = &err {
            if response.status_code == StatusCode::FORBIDDEN
                || response.error.code == JsonErrorCode::LackPermissionsForAction
            {
                return Self::Forbidden;
            }
        }
        Self::Other(err.to_string())
    }
}

#[async_trait]
pub trait AuditSource: Send + Sync {
    /// The newest `limit` entries of one action type, newest first.
    async fn recent(
        &self,
        guild: GuildId,
        kind: AuditKind,
        limit: u8,
    ) -> Result<Vec<AuditRow>, AuditError>;
}

/// The Discord audit-log action of a kind.
fn action_of(kind: AuditKind) -> Action {
    match kind {
        AuditKind::Disconnect => Action::Member(MemberAction::MemberDisconnect),
        AuditKind::Move => Action::Member(MemberAction::MemberMove),
    }
}

/// An id Discord could not have meant: 0 (`u64::MAX` cannot reach here,
/// serenity's ids are non-max).
pub(super) fn nonzero(id: u64) -> Option<u64> {
    (id != 0).then_some(id)
}

/// One entry as an [`AuditRow`] of `kind`; `None` for an entry of another
/// action or with a zero id. A missing count is one action.
pub fn row_of(kind: AuditKind, e: &AuditLogEntry) -> Option<AuditRow> {
    if e.action.num() != action_of(kind).num() {
        return None;
    }
    let id = nonzero(e.id.get())?;
    let options = e.options.as_ref();
    Some(AuditRow {
        id,
        kind,
        actor: e.user_id.and_then(|u| nonzero(u.get())).map(UserId::new),
        target: e.target_id.and_then(|t| nonzero(t.get())).map(UserId::new),
        count: options
            .and_then(|o| o.count)
            .map_or(1, |c| u32::try_from(c.get()).unwrap_or(u32::MAX)),
        channel: options
            .and_then(|o| o.channel_id)
            .and_then(|c| nonzero(c.get()))
            .map(ChannelId::new),
    })
}

/// The real audit log behind [`AuditSource`]. Untested here beyond
/// [`row_of`] and the error mapping; the TuneTitan checklist covers it.
pub struct LiveAudit {
    pub http: Arc<Http>,
}

#[async_trait]
impl AuditSource for LiveAudit {
    async fn recent(
        &self,
        guild: GuildId,
        kind: AuditKind,
        limit: u8,
    ) -> Result<Vec<AuditRow>, AuditError> {
        let logs = self
            .http
            .get_audit_logs(
                guild,
                Some(action_of(kind).num()),
                None,
                None,
                None,
                NonMaxU8::new(limit),
            )
            .await?;
        Ok(logs
            .entries
            .iter()
            .filter_map(|e| row_of(kind, e))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::http::ErrorResponse;

    async fn http_error(status: u16, code: u32) -> serenity::Error {
        let body = format!(r#"{{"code": {code}, "message": "nope"}}"#);
        let response = reqwest13::Response::from(
            http::Response::builder()
                .status(status)
                .body(body)
                .expect("a valid response"),
        );
        serenity::Error::Http(HttpError::UnsuccessfulRequest(
            ErrorResponse::from_response(response, http::Method::GET).await,
        ))
    }

    #[tokio::test]
    async fn audit_error_maps_403_and_50013_to_forbidden() {
        assert_eq!(
            AuditError::from(http_error(403, 50013).await),
            AuditError::Forbidden
        );
        assert_eq!(
            AuditError::from(http_error(403, 50001).await),
            AuditError::Forbidden,
            "any 403"
        );
        assert_eq!(
            AuditError::from(http_error(400, 50013).await),
            AuditError::Forbidden,
            "50013 whatever the status"
        );
        match AuditError::from(http_error(500, 0).await) {
            AuditError::Other(text) => assert!(!text.is_empty()),
            other => panic!("expected Other, got {other:?}"),
        }
    }

    fn entry(json: &str) -> AuditLogEntry {
        serde_json::from_str::<AuditLogEntry>(json).expect("an audit log entry")
    }

    #[test]
    fn a_disconnect_entry_becomes_a_row_and_a_missing_count_is_one() {
        let e = entry(r#"{"id": "500", "action_type": 27, "user_id": "7"}"#);
        assert_eq!(
            row_of(AuditKind::Disconnect, &e),
            Some(AuditRow {
                id: 500,
                kind: AuditKind::Disconnect,
                actor: Some(UserId::new(7)),
                target: None,
                count: 1,
                channel: None,
            })
        );
    }

    #[test]
    fn a_move_entry_carries_its_count_destination_and_target() {
        let e = entry(
            r#"{"id": "501", "action_type": 26, "user_id": "7", "target_id": "8",
                "options": {"count": "3", "channel_id": "900"}}"#,
        );
        assert_eq!(
            row_of(AuditKind::Move, &e),
            Some(AuditRow {
                id: 501,
                kind: AuditKind::Move,
                actor: Some(UserId::new(7)),
                target: Some(UserId::new(8)),
                count: 3,
                channel: Some(ChannelId::new(900)),
            })
        );
    }

    #[test]
    fn an_entry_of_another_action_or_a_zero_id_is_no_row() {
        let mv = entry(r#"{"id": "501", "action_type": 26, "user_id": "7"}"#);
        assert_eq!(
            row_of(AuditKind::Disconnect, &mv),
            None,
            "a move is no disconnect"
        );
        let zero = entry(r#"{"id": "0", "action_type": 27, "user_id": "7"}"#);
        assert_eq!(row_of(AuditKind::Disconnect, &zero), None);
        let no_actor = entry(r#"{"id": "5", "action_type": 27, "user_id": "0"}"#);
        assert_eq!(
            row_of(AuditKind::Disconnect, &no_actor).and_then(|r| r.actor),
            None,
            "a zero actor is none"
        );
    }
}
