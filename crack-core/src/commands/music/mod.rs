pub mod autopause;
pub mod autoplay;
pub mod clear;
pub mod collector;
pub mod doplay;
pub mod dosearch;
pub mod gambling;
pub mod get_metadata;
pub mod grab;
pub mod leave;
pub mod lyrics;
pub mod manage_sources;
pub mod nowplaying;
pub mod pause;
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
pub use clear::*;
pub use collector::*;
pub use doplay::*;
pub use gambling::*;
pub use get_metadata::*;
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

pub fn music_commands() -> Vec<crate::Command> {
    if cfg!(feature = "crack-music") {
        vec![
            autopause(),
            autoplay(),
            clear(),
            grab(),
            leave(),
            lyrics(),
            nowplaying(),
            optplay(),
            pause(),
            play(),
            playfile(),
            playlog(),
            playnext(),
            playytplaylist(),
            queue(),
            remove(),
            repeat(),
            resume(),
            search(),
            seek(),
            shuffle(),
            movesong(),
            skip(),
            stop(),
            summon::summon(),
            summonchannel(),
            volume(),
            vote(),
            voteskip(),
            get_metadata(),
        ]
    } else {
        vec![]
    }
}

/// Get the game commands.
pub fn game_commands() -> Vec<crate::Command> {
    if cfg!(feature = "crack-music") {
        vec![coinflip(), rolldice()]
    } else {
        vec![]
    }
}
