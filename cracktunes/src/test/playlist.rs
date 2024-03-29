#[cfg(test)]
mod test {
    use async_trait::async_trait;
    use crack_core::db::{
        playlist::{Playlist, PlaylistTrack},
        Metadata,
    };
    use mockall::automock;
    use sqlx::PgPool;
    use std::env;

    #[cfg_attr(test, automock)]
    #[async_trait]
    pub trait Database {
        async fn create_metadata(&self, in_metadata: Metadata) -> Result<Metadata, sqlx::Error>;
        async fn create_playlist(&self, name: &str, user_id: i64) -> Result<Playlist, sqlx::Error>;
        // other database functions
        async fn get_playlist_by_id(&self, playlist_id: i32) -> Result<Playlist, sqlx::Error>;
        async fn update_playlist_name(
            &self,
            playlist_id: i32,
            new_name: String,
        ) -> Result<Playlist, sqlx::Error>;
        async fn delete_playlist(&self, playlist_id: i32) -> Result<u64, sqlx::Error>;
        async fn add_track(
            &self,
            playlist_id: i32,
            metadata_id: i32,
            guild_id: i64,
            channel_id: i64,
        ) -> Result<(), sqlx::Error>;
        async fn delete_track(
            &self,
            playlist_id: i32,
            metadata_id: i32,
            guild_id: i64,
            channel_id: i64,
        ) -> Result<u64, sqlx::Error>;
        async fn get_tracks(
            &self,
            playlist_id: i32,
            guild_id: i64,
            channel_id: i64,
        ) -> Result<Vec<PlaylistTrack>, sqlx::Error>;
    }

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[test]
    fn set_env() {
        env::set_var(
            "DATABASE_URL",
            "postgresql://postgres:mysecretpassword@localhost:5432/postgres",
        );
    }

    #[tokio::test]
    async fn test_create_playlist() {
        let mut mock_db = MockDatabase::new();
        mock_db.expect_create_playlist().returning(|name, user_id| {
            Ok(Playlist {
                id: 1,
                name: name.to_string(),
                user_id: Some(user_id),
                privacy: "public".to_string(),
            })
        });

        let playlist = mock_db.create_playlist("Test Playlist", 1).await.unwrap();
        assert_eq!(playlist.name, "Test Playlist");
    }

    #[tokio::test]
    async fn test_get_playlist_by_id() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_playlist_by_id()
            .returning(|playlist_id| {
                Ok(Playlist {
                    id: playlist_id,
                    name: "Test Playlist".to_string(),
                    user_id: Some(1),
                    privacy: "private".to_string(),
                })
            });

        let playlist = mock_db.get_playlist_by_id(1).await.unwrap();
        assert_eq!(playlist.name, "Test Playlist");
    }

    #[tokio::test]
    async fn test_update_playlist_name() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_update_playlist_name()
            .returning(|playlist_id, new_name| {
                Ok(Playlist {
                    id: playlist_id,
                    name: new_name,
                    user_id: Some(1),
                    privacy: "private".to_string(),
                })
            });

        let playlist = mock_db
            .update_playlist_name(1, "Updated Playlist".to_string())
            .await
            .unwrap();
        assert_eq!(playlist.name, "Updated Playlist");
    }

    #[tokio::test]
    async fn test_delete_playlist() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_delete_playlist()
            .returning(|playlist_id| Ok(playlist_id as u64));

        let delete_count = mock_db.delete_playlist(1).await.unwrap();
        assert_eq!(delete_count, 1);
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
        assert!(!res.is_ok(), "Playlist was not deleted successfully");

        //Ok(())
    }

    #[tokio::test]
    async fn test_add_track() {
        let mut mock_db = MockDatabase::new();
        mock_db.expect_add_track().returning(|_, _, _, _| Ok(()));

        let add_track = mock_db
            .add_track(1, 1, 1, 1)
            .await
            .expect("Failed to add track");
        assert_eq!(add_track, ());
    }

    #[tokio::test]
    async fn test_delete_track() {
        let mut mock_db = MockDatabase::new();
        mock_db.expect_delete_track().returning(|_, _, _, _| Ok(1));

        let delete_count = mock_db
            .delete_track(1, 1, 1, 1)
            .await
            .expect("Failed to delete track");
        assert_eq!(delete_count, 1);
    }

    #[tokio::test]
    async fn test_get_tracks() {
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_get_tracks()
            .returning(|_, _, _| Ok(vec![PlaylistTrack::default()]));

        let tracks = mock_db
            .get_tracks(1, 1, 1)
            .await
            .expect("Failed to get tracks");
        assert_eq!(tracks.len(), 1);
    }

    #[tokio::test]
    async fn test_create_metadata() {
        let metadata = Metadata::default();
        let mut mock_db = MockDatabase::new();
        mock_db
            .expect_create_metadata()
            .returning(|_| Ok(Metadata::default()));

        let metadata_out = mock_db
            .create_metadata(metadata.clone())
            .await
            .expect("Failed to create metadata");
        assert_eq!(metadata_out.id, metadata.id);
    }
}
