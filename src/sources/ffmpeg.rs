use crate::Error;
use poise::serenity_prelude as prelude;
use songbird::input::{Codec, Container, Input, Metadata, Reader};
use std::{
    io::Write,
    process::{Child, Command, Stdio},
};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use url::Url;

pub async fn ffmpeg(
    mut source: Child,
    metadata: Metadata,
    pre_args: &[&str],
) -> Result<Input, Error> {
    let ffmpeg_args = [
        "-i",
        "-", // read from stdout
        "-f",
        "s16le", // use PCM signed 16-bit little-endian format
        "-ac",
        "2", // set two audio channels
        "-ar",
        "48000", // set audio sample rate of 48000Hz
        "-acodec",
        "pcm_f32le",
        "-",
    ];

    let taken_stdout = source
        .stdout
        .take()
        .ok_or(songbird::input::error::Error::Stdout)?;

    tracing::warn!("taken_stdout: {:?}", taken_stdout);

    let ffmpeg = Command::new("ffmpeg")
        .args(pre_args)
        .args(ffmpeg_args)
        .stdin(taken_stdout)
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;

    tracing::warn!("ffmpeg: {:?}", ffmpeg);

    let reader = Reader::from(vec![source, ffmpeg]);

    let input = Input::new(
        true,
        reader,
        Codec::FloatPcm,
        Container::Raw,
        Some(metadata),
    );

    tracing::warn!("input: {:?}", input);

    Ok(input)
}

pub async fn download(url: &str) -> Result<Vec<u8>, Error> {
    let bytes = reqwest::get(url).await?.bytes().await?;
    Ok(bytes.to_vec())
}

pub async fn from_attachment(
    att: prelude::Attachment,
    metadata: Metadata,
    pre_args: &[&str],
) -> Result<Input, Error> {
    let data = att.download().await.unwrap();
    from_bytes(data, metadata, pre_args).await
}

pub async fn from_uri(uri: &str, metadata: Metadata, pre_args: &[&str]) -> Result<Input, Error> {
    let data = download(uri).await.unwrap();

    tracing::warn!("Downloaded {} bytes from {}", data.len(), uri);

    let url = Url::parse(uri).unwrap();
    let file_name = url.path_segments().unwrap().last().unwrap();

    tracing::warn!("File name: {}", file_name);

    File::create(file_name)
        .await
        .unwrap()
        .write(&data)
        .await
        .unwrap();

    let child = Command::new("cat")
        .arg(file_name)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    // child
    //     .stdin
    //     .as_mut()
    //     .ok_or("Failed to open stdin")?
    //     .write_all(&data)?;

    tracing::warn!("Spawned cat");
    ffmpeg(child, metadata, pre_args).await
}

pub async fn from_bytes(
    data: Vec<u8>,
    metadata: Metadata,
    pre_args: &[&str],
) -> Result<Input, Error> {
    let ffmpeg_args = [
        "-i",
        "-", // read from stdout
        "-f",
        "s16le", // use PCM signed 16-bit little-endian format
        "-ac",
        "2", // set two audio channels
        "-ar",
        "48000", // set audio sample rate of 48000Hz
        "-acodec",
        "pcm_f32le",
        "-",
    ];

    let bytes = data;

    // Command::new()
    // let taken_stdout = source.stdout.take().ok_or(Error::Stdout)?;

    let mut ffmpeg = Command::new("ffmpeg")
        .args(pre_args)
        .args(ffmpeg_args)
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;

    let writer = match ffmpeg.stdin.as_mut().ok_or("Failed to open stdin") {
        Ok(writer) => writer,
        Err(x) => {
            tracing::error!(x);
            return Err(songbird::input::error::Error::Stdout.into());
        }
    };

    let _ = writer.write_all(&bytes);

    //let output = ffmpeg.wait_with_output()?;

    let reader = Reader::from(vec![ffmpeg]);

    let input = Input::new(
        true,
        reader,
        Codec::FloatPcm,
        Container::Raw,
        Some(metadata),
    );

    Ok(input)
}

// pub async fn ffprobe(attachment: Attachment) -> Result<Metadata> {
//     let mut ffprobe = Command::new("ffprobe")
//         .args(&[
//             "-v",
//             "quiet",
//             "-print_format",
//             "json",
//             "-show_format",
//             "-show_streams",
//             "-",
//         ])
//         .stdin(Stdio::piped())
//         .stderr(Stdio::null())
//         .stdout(Stdio::piped())
//         .spawn()?;

//     let writer = match ffprobe.stdin.as_mut().ok_or("Failed to open stdin") {
//         Ok(writer) => writer,
//         Err(x) => {
//             tracing::error!(x);
//             return Err(Error::Stdout)
//         },
//     };

//     let bytes = attachment.download().await.unwrap();

//     let _ = writer.write_all(&bytes);

//     let output = ffprobe.wait_with_output()?;

//     let metadata = Metadata::from_ffprobe_json(&serde_json::from_slice(&output.stdout)?);

//     Ok(metadata)
// }
