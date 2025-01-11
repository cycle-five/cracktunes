#[cfg(test)]
mod test {
    use crack_core::db::playlist::Playlist;
    use sqlx::PgPool;
    use std::env;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[test]
    fn set_env() {
        env::set_var(
            "DATABASE_URL",
            "postgresql://postgres:mysecretpassword@localhost:5432/postgres",
        );
    }

    //#[tokio::test]
    #[sqlx::test(migrator = "MIGRATOR")]
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
        assert!(res.is_err(), "Playlist was not deleted successfully");

        //Ok(())
    }
}
