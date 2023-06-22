use self::serenity::async_trait;
use crate::commands::play::QueryType;
use ::serenity::json::Value;
use poise::serenity_prelude as serenity;
use songbird::input::{
    error::Result as SongbirdResult, restartable::Restart, Codec, Container, Input, Metadata,
    Restartable,
};
use std::time::Duration;
use url::Url;

use super::ffmpeg;

pub struct FileSource {}

impl FileSource {
    pub fn extract(query: serenity::Attachment) -> Option<QueryType> {
        Some(QueryType::File(query))
    }
}

pub struct FileRestable {}

impl FileRestable {
    pub async fn download<P: AsRef<str> + Send + Clone + Sync + 'static>(
        uri: P,
        attachment: serenity::Attachment,
        lazy: bool,
    ) -> SongbirdResult<Restartable> {
        Restartable::new(FileRestarter { attachment, uri }, lazy).await
    }
}

struct FileRestarter<P>
where
    P: AsRef<str> + Send + Sync,
{
    attachment: serenity::Attachment,
}

#[async_trait]
impl<P> Restart for FileRestarter<P>
where
    P: AsRef<str> + Send + Clone + Sync,
{
    async fn call_restart(&mut self, time: Option<Duration>) -> SongbirdResult<Input> {
        // let (yt, metadata) = ytdl(self.uri.as_ref()).await?;
        let attachment = self.attachment.clone();

        let Some(time) = time else {
            return ffmpeg::from_attachment(attachment, Metadata::default(), &[]).await;
        };

        let ts = format!("{:.3}", time.as_secs_f64());
        ffmpeg::from_attachment(attachment, Metadata::default(), &["-ss", &ts]).await
    }

    async fn lazy_init(&mut self) -> SongbirdResult<(Option<Metadata>, Codec, Container)> {
        Ok((None, Codec::FloatPcm, Container::Raw))
    }
}

async fn _file_metadata(url: &str) -> SongbirdResult<Metadata> {
    let url = Url::parse(url).unwrap();
    let res = ffprobe::ffprobe_async_url(url).await.unwrap();

    let asdf = serde_json::to_string(&res).unwrap();
    let json_res = asdf.as_str();

    let val: Value = serde_json::from_str(json_res).unwrap();

    Ok(Metadata::from_ffprobe_json(&val))
}
