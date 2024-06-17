pub mod autopause;
pub mod autoplay;
pub mod clean;
pub mod clear;
pub mod collector;
pub mod doplay;
pub mod dosearch;
pub mod gambling;
pub mod grab;
pub mod leave;
pub mod lyrics;
pub mod manage_sources;
pub mod nowplaying;
pub mod pause;
pub mod play_utils;
pub mod playlog;
pub mod queue;
pub mod remove;
pub mod repeat;
pub mod resume;
pub mod seek;
pub mod shuffle;
pub mod skip;
pub mod stop;
pub mod summon;
pub mod volume;
pub mod vote;
pub mod voteskip;

pub use autopause::*;
pub use autoplay::*;
pub use clean::*;
pub use clear::*;
pub use collector::*;
pub use doplay::*;
pub use gambling::*;
pub use grab::*;
pub use leave::*;
pub use lyrics::*;
pub use manage_sources::*;
pub use nowplaying::*;
pub use pause::*;
pub use playlog::*;
pub use queue::*;
pub use remove::*;
pub use repeat::*;
pub use resume::*;
pub use seek::*;
pub use shuffle::*;
pub use skip::*;
pub use stop::*;
pub use summon::*;
pub use volume::*;
pub use vote::*;
pub use voteskip::*;

pub fn music_commands() -> [crate::Command; 23] {
    [
        autopause(),
        autoplay(),
        clean(),
        clear(),
        grab(),
        lyrics(),
        nowplaying(),
        pause(),
        play(),
        playlog(),
        playnext(),
        queue(),
        remove(),
        repeat(),
        resume(),
        seek(),
        shuffle(),
        skip(),
        stop(),
        summon(),
        volume(),
        vote(),
        voteskip(),
    ]
}
