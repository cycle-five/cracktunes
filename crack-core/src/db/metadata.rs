use chrono::Duration;
use sqlx::{postgres::types::PgInterval, PgPool};

use crate::errors::CrackedError;

#[derive(Debug, Default, Clone)]
pub struct Metadata {
    pub id: i32,
    pub track: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub date: Option<chrono::NaiveDate>,
    pub channels: Option<i16>,
    pub channel: Option<String>,
    pub start_time: Option<Duration>,
    pub duration: Option<Duration>,
    pub sample_rate: Option<i32>,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
}

pub struct MetadataRead {
    pub id: i32,
    pub track: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub date: Option<chrono::NaiveDate>,
    pub channels: Option<i16>,
    pub channel: Option<String>,
    pub start_time: Option<PgInterval>,
    pub duration: Option<PgInterval>,
    pub sample_rate: Option<i32>,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
}

impl Metadata {
    pub async fn create(pool: &PgPool, in_metadata: Metadata) -> Result<Metadata, CrackedError> {
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
            in_metadata.start_time.map(|x| PgInterval::try_from(x.to_std().unwrap()).unwrap()),
            in_metadata.duration.map(|x| PgInterval::try_from(x.to_std().unwrap()).unwrap()),
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
            start_time: r.start_time.map(|x| {
                Duration::from_std(std::time::Duration::from_micros(
                    x.microseconds.unsigned_abs(),
                ))
                .unwrap()
            }),
            duration: r.duration.map(|x| {
                Duration::from_std(std::time::Duration::from_micros(
                    x.microseconds.unsigned_abs(),
                ))
                .unwrap()
            }),
            sample_rate: r.sample_rate,
            source_url: r.source_url,
            title: r.title,
            thumbnail: r.thumbnail,
        })
    }
}
