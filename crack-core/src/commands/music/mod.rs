// Original commands
pub mod autopause;
pub mod autoplay;
pub mod clear;
pub mod collector;
//pub mod doplay;
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
pub mod seek;
pub mod shuffle;
pub mod skip;
pub mod stop;
pub mod summon;
pub mod volume;
pub mod vote;
pub mod voteskip;

// Refactored modules
pub mod doplay_refactored;
pub mod resume;

pub use autopause::autopause;
pub use autoplay::{autoplay, toggle_autoplay};
pub use clear::clear;
pub use doplay_refactored::{optplay, play, playfile, playnext, playytplaylist, search};
pub use dosearch::do_yt_search;
pub use gambling::{coinflip, roll_n_d, rolldice};
pub use get_metadata::get_metadata;
pub use grab::grab;
pub use leave::leave;
pub use lyrics::lyrics;
pub use nowplaying::nowplaying;
pub use pause::pause;
pub use playlog::playlog;
pub use queue::queue;
pub use remove::remove;
pub use repeat::repeat;
pub use resume::resume;
pub use seek::seek;
pub use shuffle::{movesong, shuffle};
pub use skip::skip;
pub use stop::stop;
pub use summon::{summon, summonchannel};
pub use volume::volume;
pub use vote::vote;

#[must_use]
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
            get_metadata(),
        ]
    } else {
        vec![]
    }
}

/// Get the game commands.
#[must_use]
pub fn game_commands() -> Vec<crate::Command> {
    if cfg!(feature = "crack-music") {
        vec![coinflip(), rolldice()]
    } else {
        vec![]
    }
}
