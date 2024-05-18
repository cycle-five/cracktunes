use std::sync::Arc;
// use std::{mem, slice};

use songbird::{Call, CoreEvent};
use tokio::fs::File;
use tokio::io::AsyncWriteExt as TokAsyncWriteExt; // for write_all()

use serenity::async_trait;
use serenity::prelude::RwLock;

use serenity::client::EventHandler;

use songbird::{
    model::payload::{ClientDisconnect, Speaking},
    Event, EventContext, EventHandler as VoiceEventHandler,
};

use serenity::{client::Context as SerenityContext, model::gateway::Ready};
use std::time::{SystemTime, UNIX_EPOCH};

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

pub struct Receiver {
    pub data: Arc<RwLock<Vec<u8>>>,
}

impl Receiver {
    pub fn new(arc: Arc<RwLock<Vec<u8>>>) -> Self {
        // Copy of the global audio buffer with RWLock
        Self { data: arc }
    }

    // FIXME
    #[allow(dead_code)]
    async fn insert(&self, buf: &[u8]) {
        let insert_lock = {
            // While data is a RwLock, it's recommended that you always open the lock as read.
            // This is mainly done to avoid Deadlocks for having a possible writer waiting for multiple
            // readers to close.
            self.data.write().await
            //let data_read = self.data.write().await;

            //data_read
            // data, instead the reference is cloned.
            // We wrap every value on in an Arc, as to keep the data lock open for the least time possible,
            // to again, avoid deadlocking it.
            // data_read
            //     .get::<AudioBuffer>()
            //     .expect("Expected AudioBuffer.")
            //     .clone()
        };

        // Just like with client.data in main, we want to keep write locks open the least time
        // possible, so we wrap them on a block so they get automatically closed at the end.
        {
            // The HashMap of CommandCounter is wrapped in an RwLock; since we want to write to it, we will
            // open the lock in write mode.
            // let mut buff = insert_lock.write().await;
            let mut buff = insert_lock; //.clone();

            println!("AudioBuffer size: {}", buff.len());
            // And we write the amount of times the command has been called to it.
            if buff.len() > 100000000 {
                let mut out_file = File::create(format!(
                    "file_{:?}.out",
                    SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
                ))
                .await
                .unwrap();
                out_file.write_all(&buff).await.unwrap();
                buff.clear();
            }
            //
            buff.extend_from_slice(buf);
        }
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
                // if let Some(audio) = data.audio {
                //     // FIXME: Can we not do an unsafe?
                //     let slice_u8: &[u8] = unsafe {
                //         slice::from_raw_parts(
                //             audio.as_ptr() as *const u8,
                //             audio.len() * mem::size_of::<u16>(),
                //         )
                //     };
                //     // self.insert(slice_u8);
                //     self.insert(slice_u8).await;

                //     println!(
                //         "Audio packet's first 5 samples: {:?}",
                //         audio.get(..5.min(audio.len()))
                //     );
                //     println!(
                //         "Audio packet sequence {:05} has {:04} bytes (decompressed from {}), SSRC {}",
                //         data.packet.sequence.0,
                //         audio.len() * std::mem::size_of::<i16>(),
                //         data.packet.payload.len(),
                //         data.packet.ssrc,
                //     );
                // } else {
                //     println!("RTP packet, but no audio. Driver may not be configured to decode.");
                // }
                tracing::trace!("RTP packet received: {:?}", data.packet);
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

                tracing::warn!("Client disconnected: user {:?}", user_id);
            },
            Ctx::Track(track_data) => {
                // An event which fires when a new track starts playing.
                if track_data.is_empty() {
                    return None;
                }
                tracing::warn!("{:?}", track_data);
                for &(track_state, track_handle) in track_data.iter() {
                    tracing::warn!(
                        "Track started: {:?} (handle: {:?})",
                        track_state,
                        track_handle,
                    );
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

pub async fn register_voice_handlers(
    buffer: Arc<RwLock<Vec<u8>>>,
    handler_lock: Arc<tokio::sync::Mutex<Call>>,
) -> Result<(), CrackedError> {
    // NOTE: this skips listening for the actual connection result.
    let mut handler = handler_lock.lock().await;
    // .map_err(|e| {
    //     tracing::error!("Error locking handler: {:?}", e);
    //     CrackedError::RSpotifyLockError(format!("{e:?}"))
    // })?;

    handler.add_global_event(
        CoreEvent::SpeakingStateUpdate.into(),
        Receiver::new(buffer.clone()),
    );

    // handler.add_global_event(
    //     CoreEvent::SpeakingStateUpdate.into(),
    //     Receiver::new(buffer.clone()),
    // );

    handler.add_global_event(CoreEvent::RtpPacket.into(), Receiver::new(buffer.clone()));

    handler.add_global_event(CoreEvent::RtcpPacket.into(), Receiver::new(buffer.clone()));

    handler.add_global_event(
        CoreEvent::ClientDisconnect.into(),
        Receiver::new(buffer.clone()),
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
        let receiver = Receiver::new(buffer.clone());
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
        // let buf = buf.as_slice();
        // let mut read_buf = [0 as u8; 8];
        // read_buf.copy_from_slice(&buf[..8]);
        let user_id = u64::from_be_bytes(buf.as_slice().try_into().unwrap());
        let got = VoiceUserId(user_id);

        assert_eq!(want, got);
    }
}
