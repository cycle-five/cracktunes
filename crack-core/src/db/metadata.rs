use serde::{Deserialize, Serialize};
use songbird::input::AuxMetadata;
use sqlx::PgPool;

use crate::errors::CrackedError;

// #[serde_with::serde_as]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub id: i32,
    pub track: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub date: Option<chrono::NaiveDate>,
    pub channels: Option<i16>,
    pub channel: Option<String>,
    pub start_time: i64,
    pub duration: i64,
    pub sample_rate: Option<i32>,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct MetadataRead {
    pub id: i32,
    pub track: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub date: Option<chrono::NaiveDate>,
    pub channels: Option<i16>,
    pub channel: Option<String>,
    pub start_time: i64,
    pub duration: i64,
    pub sample_rate: Option<i32>,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
}

impl Metadata {
    pub async fn create(pool: &PgPool, in_metadata: &Metadata) -> Result<Metadata, CrackedError> {
        let r = sqlx::query_as!(
            MetadataRead,
            r#"INSERT INTO
                metadata (track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                RETURNING id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
            "#,
            in_metadata.track,
            in_metadata.artist,
            in_metadata.album,
            in_metadata.date,
            in_metadata.channels.map(|x| i16::try_from(x).unwrap()),
            in_metadata.channel,
            in_metadata.start_time, //.map(|x| PgInterval::try_from(x).unwrap()),
            in_metadata.duration, //.map(|x| PgInterval::try_from(x).unwrap()),
            in_metadata.sample_rate,
            in_metadata.source_url,
            in_metadata.title,
            in_metadata.thumbnail,
        )
        .fetch_one(pool)
        .await
        .map_err(CrackedError::SQLX)?;
        Ok(Metadata {
            id: r.id,
            track: r.track,
            artist: r.artist,
            album: r.album,
            date: r.date,
            channels: r.channels,
            channel: r.channel,
            start_time: r.start_time,
            duration: r.duration,
            sample_rate: r.sample_rate,
            source_url: r.source_url,
            title: r.title,
            thumbnail: r.thumbnail,
        })
    }
}

use crate::db;
/// Convert an `AuxMetadata` structure to the database structures.
pub fn aux_metadata_to_db_structures(
    metadata: &AuxMetadata,
    guild_id: i64,
    channel_id: i64,
) -> Result<(Metadata, db::PlaylistTrack), CrackedError> {
    let track = metadata.track.clone();
    let title = metadata.title.clone();
    let artist = metadata.artist.clone();
    let album = metadata.album.clone();
    let date = metadata
        .date
        .as_ref()
        .map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap_or_default());
    let channel = metadata.channel.clone();
    let channels = metadata.channels.map(i16::from);
    let start_time = metadata
        .start_time
        .map(|d| d.as_secs_f64() as i64)
        .unwrap_or(0);
    let duration = metadata
        .duration
        .map(|d| d.as_secs_f64() as i64)
        .unwrap_or(0);
    let sample_rate = metadata.sample_rate.map(|d| i64::from(d) as i32);
    let thumbnail = metadata.thumbnail.clone();
    let source_url = metadata.source_url.clone();

    let metadata = Metadata {
        id: 0,
        track,
        title,
        artist,
        album,
        date,
        channel,
        channels,
        start_time,
        duration,
        sample_rate,
        source_url,
        thumbnail,
    };

    let db_track = db::PlaylistTrack {
        id: 0,
        playlist_id: 0,
        guild_id: Some(guild_id),
        metadata_id: 0,
        channel_id: Some(channel_id),
    };

    Ok((metadata, db_track))
}
