/*
use self::serenity::async_trait;
use crate::Error;
use crate::{commands::play::QueryType, errors::CrackedError};
use ::serenity::json::Value;
use audiopus::Result as SongbirdResult;
use poise::serenity_prelude as serenity;
use songbird::input::{Input, Metadata};
use std::time::Duration;
use url::Url;

use super::ffmpeg;

pub struct FileSource {}

impl FileSource {
    pub fn extract(query: serenity::Attachment) -> Option<QueryType> {
        Some(QueryType::File(query))
    }
}

pub struct FileRestartable {}

impl FileRestartable {
    pub async fn download<P: AsRef<str> + Send + Clone + Sync + 'static>(
        uri: P,
        lazy: bool,
    ) -> SongbirdResult<Restartable> {
        Restartable::new(FileRestarter { uri }, lazy).await
    }
}

struct FileRestarter<P>
where
    P: AsRef<str> + Send + Sync,
{
    uri: P,
}

#[async_trait]
impl<P> Restart for FileRestarter<P>
where
    P: AsRef<str> + Send + Clone + Sync,
{
    async fn call_restart(&mut self, time: Option<Duration>) -> SongbirdResult<Input> {
        // let (yt, metadata) = ytdl(self.uri.as_ref()).await?;
        tracing::info!("Restarting file source: {}", self.uri.as_ref());
        let url = self.uri.as_ref();

        let Some(time) = time else {
            //attachment::download().await;
            //return ffmpeg::from_attachment(attachment, Metadata::default(), &[]).await;
            let metadata = _file_metadata(url).await?;
            tracing::warn!("metadata: {:?}", metadata);
            return ffmpeg::from_uri(url, metadata, &[])
                .await
                .map_err(|e: Error| Into::<CrackedError>::into(e).into());
        };

        let ts = format!("{:.3}", time.as_secs_f64());
        ffmpeg::from_uri(url, Metadata::default(), &["-ss", &ts])
            .await
            .map_err(|e: Error| Into::<CrackedError>::into(e).into())
    }

    async fn lazy_init(&mut self) -> SongbirdResult<(Option<Metadata>, Codec, Container)> {
        let url = self.uri.as_ref();
        _file_metadata(url)
            .await
            .map(|m| (Some(m), Codec::FloatPcm, Container::Raw))
    }
}

async fn _file_metadata(url: &str) -> SongbirdResult<Metadata> {
    let url_parsed = Url::parse(url).unwrap();
    let res = ffprobe::ffprobe_async_url(url_parsed.clone())
        .await
        .unwrap();

    let asdf = serde_json::to_string(&res).unwrap();
    let json_res = asdf.as_str();

    let val: Value = serde_json::from_str(json_res).unwrap();
    tracing::warn!("ffprobe result: {:?}", val);

    let mut metadata = Metadata::from_ffprobe_json(&val);
    tracing::warn!("metadata: {:?}", metadata);

    metadata.source_url = Some(url.to_string());
    metadata.title = Some(
        url_parsed
            .path_segments()
            .unwrap()
            .last()
            .unwrap()
            .to_string(),
    );

    Ok(metadata)
}
*/
