use serenity::all::{Cache, CacheHttp, Http};
use serenity::async_trait;
use serenity::client::EventHandler;
use serenity::prelude::RwLock;
use serenity::{client::Context as SerenityContext, model::gateway::Ready};
use songbird::tracks::PlayMode;
use songbird::{
    model::payload::{ClientDisconnect, Speaking},
    Event, EventContext, EventHandler as VoiceEventHandler,
};
use songbird::{Call, CoreEvent, TrackEvent};
use std::{mem, slice, sync::Arc};

use crate::errors::CrackedError;

// use crate::{Context, Error};
// use typemap_rev::TypeMap;

#[allow(dead_code)]
struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: SerenityContext, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

// struct AudioBuffer;

// impl TypeMapKey for AudioBuffer {
//     type Value = Vec<u8>;
// }

// 10MB (10s not powers of 2 since it gets stored on disk)
const DEFAULT_BUFFER_SIZE: usize = 100_000_000;

pub struct Receiver {
    pub data: Arc<RwLock<Vec<u8>>>,
    pub cache: Option<Arc<Cache>>,
    pub http: Arc<Http>,
    buf_size: usize,
}

impl Receiver {
    pub fn new(data: Arc<RwLock<Vec<u8>>>, ctx: Option<SerenityContext>) -> Self {
        Self {
            data,
            cache: ctx.clone().map(|x| x.cache().cloned()).unwrap_or_default(),
            http: ctx
                .map(|x| x.http.clone())
                .unwrap_or(Arc::new(Http::new(""))),
            buf_size: DEFAULT_BUFFER_SIZE,
        }
    }

    // Insert a buffer from the audio stream to the handlers internal buffer.
    // Clear it every so often to bound the memory usage.
    async fn insert(&self, buf: &[u8]) {
        let mut lock = self.data.write().await;

        let n = lock.len();
        tracing::trace!("AudioBuffer size: {}", n);
        if n > self.buf_size {
            // let mut out_file = File::create(format!(
            //     "file_{:?}.out",
            //     SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
            // ))
            // .await
            // .unwrap();
            // use tokio::io::AsyncWriteExt as TokAsyncWriteExt; // for write_all()
            // out_file.write_all(&buff).await.unwrap();
            lock.clear();
        }
        lock.extend_from_slice(buf);
    }
}

#[async_trait]
impl VoiceEventHandler for Receiver {
    #[allow(unused_variables)]
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        use EventContext as Ctx;
        match ctx {
            Ctx::SpeakingStateUpdate(Speaking {
                delay,
                speaking,
                ssrc,
                user_id,
            }) => {
                // Discord voice calls use RTP, where every sender uses a randomly allocated
                // *Synchronisation Source* (SSRC) to allow receivers to tell which audio
                // stream a received packet belongs to. As this number is not derived from
                // the sender's user_id, only Discord Voice Gateway messages like this one
                // inform us about which random SSRC a user has been allocated. Future voice
                // packets will contain *only* the SSRC.
                //
                // You can implement logic here so that you can differentiate users'
                // SSRCs and map the SSRC to the User ID and maintain this state.
                // Using this map, you can map the `ssrc` in `voice_packet`
                // to the user ID and handle their audio packets separately.
                tracing::warn!(
                    "Speaking state update: user {:?} has SSRC {:?}, using {:?}",
                    user_id,
                    ssrc,
                    speaking,
                );

                let user_id = user_id.unwrap().0.to_be_bytes();
                self.data.write().await.extend_from_slice(&user_id);

                // You can implement logic here which reacts to a user starting
                // or stopping speaking, and to map their SSRC to User ID.
                tracing::warn!(
                    "Source {} has {} speaking.",
                    ssrc,
                    if speaking.microphone() || speaking.soundshare() || speaking.priority() {
                        "started"
                    } else {
                        "stopped"
                    },
                );
            },
            Ctx::RtpPacket(data) => {
                // FIXME: update this to the new library
                // An event which fires for every received audio packet,
                // containing the decoded data.
                let ssrc = data.rtp().get_ssrc();
                let n = data.packet.len();
                let (beg, end) = data.packet.split_at(n - data.payload_end_pad);
                // Can we not do an unsafe?...
                // Seven months later... We need to do an unsafe because we are not moving
                // this memory, but reinterpreting the ptr as a slice of a different size.
                // This is I believe analogous to  `new_type *new_obj = *(new_type *)(void*)&obj;`
                // in C.
                let slice_u8: &[u8] = unsafe {
                    slice::from_raw_parts(beg.as_ptr(), beg.len() * mem::size_of::<u16>())
                };
                self.insert(slice_u8).await;

                // println!(
                //     "Audio packet's first 5 samples: {:?}",
                //     data.packet.get(..5.min(beg.len()))
                // );
                // tracing::trace!("RTP packet received: {:?}", data.packet);
            },
            Ctx::RtcpPacket(data) => {
                // An event which fires for every received rtcp packet,
                // containing the call statistics and reporting information.
                tracing::trace!("RTCP packet received: {:?}", data.packet);
            },
            Ctx::ClientDisconnect(ClientDisconnect { user_id, .. }) => {
                // You can implement your own logic here to handle a user who has left the
                // voice channel e.g., finalise processing of statistics etc.
                // You will typically need to map the User ID to their SSRC; observed when
                // first speaking.

                let user_name = tracing::warn!("Client disconnected: user {:?}", user_id);
            },
            Ctx::Track(track_data) => {
                // An event which fires when a new track starts playing.
                if track_data.is_empty() {
                    return None;
                }
                tracing::warn!("{:?}", track_data);
                for &(track_state, track_handle) in track_data.iter() {
                    match track_state.playing {
                        PlayMode::Play => {
                            tracing::warn!(
                                "Track started: {:?} (handle: {:?})",
                                track_state,
                                track_handle,
                            );
                        },
                        PlayMode::Pause => {
                            tracing::warn!(
                                "Track paused: {:?} (handle: {:?})",
                                track_state,
                                track_handle,
                            );
                        },
                        PlayMode::Stop | PlayMode::End => {
                            tracing::warn!(
                                "Track ended: {:?} (handle: {:?})",
                                track_state,
                                track_handle,
                            );
                        },
                        PlayMode::Errored(_) => {
                            tracing::warn!(
                                "Track errored: {:?} (handle: {:?})",
                                track_state,
                                track_handle,
                            );
                        },
                        _ => {
                            // There is a new variant of PlayMode that is not handled.
                            unimplemented!()
                        },
                    }
                }
            },
            Ctx::VoiceTick(_)
            | Ctx::DriverConnect(_)
            | Ctx::DriverReconnect(_)
            | Ctx::DriverDisconnect(_) => {
                // We won't be registering this struct for any more event classes.
                tracing::warn!("Event not handled: {:?}", ctx);
            },
            _ => {
                // This should not happen.
                unimplemented!()
            },
        }

        None
    }
}

/// Registers the voice handlers for a call instance for the bot.
/// These are kept per guild.
pub async fn register_voice_handlers(
    buffer: Arc<RwLock<Vec<u8>>>,
    call: Arc<tokio::sync::Mutex<Call>>,
    ctx: SerenityContext,
) -> Result<(), CrackedError> {
    // NOTE: this skips listening for the actual connection result.
    let mut handler = call.lock().await;

    // allocating memory, need to drop this when y???
    handler.add_global_event(
        CoreEvent::SpeakingStateUpdate.into(),
        Receiver::new(buffer.clone(), Some(ctx.clone())),
    );

    handler.add_global_event(
        TrackEvent::End.into(),
        Receiver::new(buffer.clone(), Some(ctx.clone())),
    );

    handler.add_global_event(
        CoreEvent::RtpPacket.into(),
        Receiver::new(buffer.clone(), Some(ctx.clone())),
    );

    handler.add_global_event(
        CoreEvent::RtcpPacket.into(),
        Receiver::new(buffer.clone(), Some(ctx.clone())),
    );

    handler.add_global_event(
        CoreEvent::ClientDisconnect.into(),
        Receiver::new(buffer.clone(), Some(ctx.clone())),
    );
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use serenity_voice_model::id::UserId as VoiceUserId;
    use songbird::model::{payload::Speaking, SpeakingState};

    #[tokio::test]
    async fn test_receiver() {
        let buffer = Arc::new(RwLock::new(Vec::new()));
        let receiver = Receiver::new(buffer.clone(), None);
        let want = VoiceUserId(0xAA);

        let speaking = Speaking {
            delay: Some(0),
            speaking: SpeakingState::MICROPHONE,
            ssrc: 0,
            user_id: Some(want),
        };

        let ctx = EventContext::SpeakingStateUpdate(speaking);
        let _ = receiver.act(&ctx).await;
        let buf = receiver.data.read().await.clone();

        let user_id = u64::from_be_bytes(buf.as_slice().try_into().unwrap());
        let got = VoiceUserId(user_id);

        assert_eq!(want, got);
    }
}
