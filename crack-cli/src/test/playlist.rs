#[cfg(test)]
mod test {
    use crack_core::db::playlist::Playlist;
    use sqlx::PgPool;
    use std::env;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    /// Point the db-tests at a database when nothing else has.
    ///
    /// 🪤 This was a `#[test]`, which made it a race rather than setup: the
    /// test harness runs tests on threads in no guaranteed order, so whether
    /// `DATABASE_URL` was set before another test read it was luck, and
    /// `set_var` alongside concurrent readers is unsound besides. A ctor runs
    /// once, before `main`, before any test thread exists.
    #[ctor::ctor(unsafe)]
    fn set_env() {
        // Kept in step with crack-core's copy by hand: the helper there lives
        // in a `#[cfg(test)]` module, which does not exist for other crates.
        const TEST_DATABASE_URL: &str =
            "postgresql://postgres:mysecretpassword@localhost:5432/postgres";
        match env::var("DATABASE_URL") {
            Ok(url) if !url.trim().is_empty() => {},
            _ => env::set_var("DATABASE_URL", TEST_DATABASE_URL),
        }
    }

    //#[tokio::test]
    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn test_delete_playlist_by_id(pool: PgPool) {
        // Setup
        let user_id = 1; // or fetch a user id for the test
        let playlist_name = "Test Playlist";

        // Create a new playlist entry
        let playlist_id = Playlist::create(&pool, playlist_name, user_id)
            .await
            .expect("Failed to create playlist");

        // Use the delete_playlist_by_id function to delete the playlist
        Playlist::delete_playlist_by_id(&pool, playlist_id.id, user_id)
            .await
            .expect("Failed to delete playlist");

        // Verify that the playlist is no longer present in the database
        let res = Playlist::get_playlist_by_id(&pool, playlist_id.id).await;
        assert!(!res.is_ok(), "Playlist was not deleted successfully");

        //Ok(())
    }
}
