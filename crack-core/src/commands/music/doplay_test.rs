use crate::{
    commands::music::doplay_refactored::{parse_mode, play_internal},
    music::queue_manager::QueuePosition,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode() {
        // Test prefix mode parsing
        assert_eq!(
            parse_mode(Some("next".to_string()), true),
            QueuePosition::Next
        );
        assert_eq!(
            parse_mode(Some("front".to_string()), true),
            QueuePosition::Front
        );
        assert_eq!(
            parse_mode(Some("search".to_string()), true),
            QueuePosition::End
        );
        assert_eq!(
            parse_mode(Some("jump".to_string()), true),
            QueuePosition::End
        );
        assert_eq!(
            parse_mode(Some("downloadmkv".to_string()), true),
            QueuePosition::End
        );
        assert_eq!(
            parse_mode(Some("downloadmp3".to_string()), true),
            QueuePosition::End
        );
        assert_eq!(
            parse_mode(Some("unknown".to_string()), true),
            QueuePosition::End
        );
        assert_eq!(parse_mode(None, true), QueuePosition::End);

        // Test slash command mode parsing
        assert_eq!(
            parse_mode(Some("next".to_string()), false),
            QueuePosition::Next
        );
        assert_eq!(
            parse_mode(Some("front".to_string()), false),
            QueuePosition::Front
        );
        assert_eq!(
            parse_mode(Some("unknown".to_string()), false),
            QueuePosition::End
        );
        assert_eq!(parse_mode(None, false), QueuePosition::End);
    }
}
