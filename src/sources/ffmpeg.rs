use poise::serenity_prelude::Attachment;
use songbird::input::{
    error::{Error, Result},
    Codec, Container, Input, Metadata, Reader,
};
use std::{
    io::Write,
    process::{Child, Command, Stdio},
};

pub async fn ffmpeg(mut source: Child, metadata: Metadata, pre_args: &[&str]) -> Result<Input> {
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

    let taken_stdout = source.stdout.take().ok_or(Error::Stdout)?;

    let ffmpeg = Command::new("ffmpeg")
        .args(pre_args)
        .args(ffmpeg_args)
        .stdin(taken_stdout)
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;

    let reader = Reader::from(vec![source, ffmpeg]);

    let input = Input::new(
        true,
        reader,
        Codec::FloatPcm,
        Container::Raw,
        Some(metadata),
    );

    Ok(input)
}

pub async fn from_attachment(
    attachment: Attachment,
    metadata: Metadata,
    pre_args: &[&str],
) -> Result<Input> {
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

    let bytes = attachment.download().await.unwrap();

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
            return Err(Error::Stdout);
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
