use std::fmt::Display;

use serde::{Deserialize, Serialize};
use songbird::input::AuxMetadata;
use sqlx::PgPool;

use crack_types::CrackedError;

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

impl Display for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        if let Some(artist) = &self.artist {
            s.push_str(&format!("{} - ", artist));
        }
        if let Some(title) = &self.title {
            s.push_str(&format!("{} - ", title));
        }
        if let Some(album) = &self.album {
            s.push_str(&format!("{} - ", album));
        }
        if let Some(track) = &self.track {
            s.push_str(&format!("{} - ", track));
        }
        if let Some(date) = &self.date {
            s.push_str(&format!("{} - ", date));
        }
        if let Some(channel) = &self.channel {
            s.push_str(&format!("{} - ", channel));
        }
        if let Some(channels) = &self.channels {
            s.push_str(&format!("{} - ", channels));
        }
        if let Some(sample_rate) = &self.sample_rate {
            s.push_str(&format!("{} - ", sample_rate));
        }
        if let Some(source_url) = &self.source_url {
            s.push_str(&format!("{} - ", source_url));
        }
        if let Some(thumbnail) = &self.thumbnail {
            s.push_str(&format!("{} - ", thumbnail));
        }
        write!(f, "{}", s)
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, sqlx::FromRow)]
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
    /// Create a metadata entry or return the existing one with id if it exists.
    pub async fn get_or_create(
        pool: &PgPool,
        in_metadata: &Metadata,
    ) -> Result<Metadata, CrackedError> {
        let get_r: Result<MetadataRead, CrackedError> = sqlx::query_as!(MetadataRead, r#"
            SELECT
                metadata.id, metadata.track, metadata.artist, metadata.album, metadata.date, metadata.channels, metadata.channel, metadata.start_time, metadata.duration, metadata.sample_rate, metadata.source_url, metadata.title, metadata.thumbnail
            FROM
                metadata
            WHERE
                metadata.source_url = $1
            "#,
                in_metadata.source_url,
            )
            .fetch_one(pool)
            .await
            .map_err(CrackedError::SQLX);
        let r = match get_r {
            Ok(r) => r,
            Err(_) => {
                sqlx::query_as!(
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
                    .map_err(CrackedError::SQLX)?
            },
        };
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

    /// Get a metadata entry by id (url).
    pub async fn get_by_url(pool: &PgPool, url: &str) -> Result<Option<Metadata>, CrackedError> {
        // let r: Option<MetadataRead> = sqlx::query_as!(MetadataRead,
        sqlx::query_as!(MetadataRead,
            r#"SELECT
                metadata.id, metadata.track, metadata.artist, metadata.album, metadata.date, metadata.channels, metadata.channel, metadata.start_time, metadata.duration, metadata.sample_rate, metadata.source_url, metadata.title, metadata.thumbnail
            FROM
                metadata
            WHERE
                metadata.source_url = $1"#,
            url
        )
        .fetch_optional(pool)
        .await
        .map_err(CrackedError::SQLX)
        .map(|r| r.map(|r| r.into()))
        // match r {
        //     Some(r) => Ok(Some(r.into())),
        //     None => Ok(None),
        // }
    }
}

impl From<MetadataRead> for Metadata {
    fn from(r: MetadataRead) -> Self {
        Metadata {
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
        }
    }
}

pub async fn playlist_track_to_metadata(
    pool: &PgPool,
    playlist_track: &PlaylistTrack,
) -> Result<Metadata, CrackedError> {
    let r: MetadataRead = sqlx::query_as!(
        MetadataRead,
        r#"SELECT
            metadata.id, metadata.track, metadata.artist, metadata.album, metadata.date, metadata.channels, metadata.channel, metadata.start_time, metadata.duration, metadata.sample_rate, metadata.source_url, metadata.title, metadata.thumbnail
            FROM metadata
            INNER JOIN playlist_track ON playlist_track.metadata_id = metadata.id
            WHERE playlist_track.id = $1
        "#,
        playlist_track.id as i32
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

use super::PlaylistTrack;
use crate::db;
/// This enum holds metadata and another type... which apprently there's going to be another use for?
/// Or maybe this was better for ergonimics?
pub enum MetadataAnd {
    Track(Metadata, PlaylistTrack),
}

/// Convert an `AuxMetadata` structure to the database structures.
pub fn aux_metadata_to_db_structures(
    metadata: &AuxMetadata,
    guild_id: i64,
    channel_id: i64,
) -> Result<MetadataAnd, CrackedError> {
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

    Ok(MetadataAnd::Track(metadata, db_track))
}

/// Convert an `AuxMetadata` structure to the database structures.
pub fn aux_metadata_from_db(metadata: &Metadata) -> Result<AuxMetadata, CrackedError> {
    let track = metadata.track.clone();
    let title = metadata.title.clone();
    let artist = metadata.artist.clone();
    let album = metadata.album.clone();
    let date = metadata.date;
    let channel = metadata.channel.clone();
    let channels = metadata.channels.map(i16::from);
    let start_time = metadata.start_time;
    let duration = metadata.duration;
    let sample_rate = metadata.sample_rate.map(|d| i64::from(d) as i32);
    let thumbnail = metadata.thumbnail.clone();
    let source_url = metadata.source_url.clone();

    let aux_metadata = AuxMetadata {
        track,
        title,
        artist,
        album,
        date: date.map(|x| x.format("%Y-%m-%d").to_string()),
        channel,
        channels: channels.map(|x| x as u8),
        start_time: Some(std::time::Duration::from_secs_f64(start_time as f64)),
        duration: Some(std::time::Duration::from_secs_f64(duration as f64)),
        sample_rate: sample_rate.map(|x| x as u32),
        source_url,
        thumbnail,
    };

    Ok(aux_metadata)
}

impl From<Metadata> for AuxMetadata {
    fn from(metadata: Metadata) -> Self {
        aux_metadata_from_db(&metadata).unwrap()
    }
}

impl From<AuxMetadata> for Metadata {
    fn from(metadata: AuxMetadata) -> Self {
        aux_metadata_to_db_structures(&metadata, 0, 0)
            .map(|MetadataAnd::Track(metadata, _)| metadata)
            .unwrap()
    }
}

impl From<AuxMetadata> for db::PlaylistTrack {
    fn from(metadata: AuxMetadata) -> Self {
        aux_metadata_to_db_structures(&metadata, 0, 0)
            .map(|MetadataAnd::Track(_, playlist_track)| playlist_track)
            .unwrap()
    }
}

impl From<AuxMetadata> for MetadataAnd {
    fn from(metadata: AuxMetadata) -> Self {
        aux_metadata_to_db_structures(&metadata, 0, 0).unwrap()
    }
}
