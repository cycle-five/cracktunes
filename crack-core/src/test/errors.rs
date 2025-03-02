#[cfg(test)]
mod test {
    use crack_types::errors::CrackedError;
    use crack_types::messaging::messages;

    #[test]
    fn test_crack_error() {
        let err = CrackedError::WrongVoiceChannel;
        assert_eq!(err.to_string(), messages::FAIL_WRONG_CHANNEL.to_string());
    }

    #[test]
    fn test_crack_error_other() {
        let err = CrackedError::Other("Test");
        assert_eq!(err.to_string(), "Test".to_string());
    }

    #[test]
    fn test_crack_error_nothing_playing() {
        let err = CrackedError::NothingPlaying;
        assert_eq!(err.to_string(), messages::FAIL_NOTHING_PLAYING.to_string());
    }

    #[test]
    fn test_crack_error_playlist_failed() {
        let err = CrackedError::PlayListFail;
        assert_eq!(err.to_string(), messages::FAIL_PLAYLIST_FETCH.to_string());
    }

    #[test]
    fn test_crack_error_parse_time_failed() {
        let err = CrackedError::ParseTimeFail;
        assert_eq!(err.to_string(), messages::FAIL_PARSE_TIME.to_string());
    }

    #[test]
    fn test_crack_error_anyhow() {
        let err = CrackedError::Anyhow(anyhow::anyhow!("Test"));
        assert_eq!(err.to_string(), "Test".to_string());
    }
}
