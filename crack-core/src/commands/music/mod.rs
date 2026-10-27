pub mod auditlog;
pub mod autopause;
pub mod autoplay;
pub mod clear;
pub mod collector;
pub mod diagnose;
pub mod doplay;
pub mod dosearch;
pub mod echoes;
pub mod ephemeral;
pub mod gambling;
pub mod get_metadata;
pub mod gp;
pub mod gp_persist {
    pub use super::gp_resume::gp_resume_guild;
    pub use crack_gp::gp_persist::{
        close_out, game_id, post_owed_results, spawn_gp_writer, take_down_components, vc_members,
        GpCloseOut, GpPersist,
    };
}
pub use crack_gp::gp_prompts;
pub mod gp_resume;
pub mod grab;
pub mod leave;
pub mod lyrics;
pub mod manage_sources;
pub mod nowplaying;
pub mod np_buttons;
pub mod pause;
pub mod playlog;
pub mod queue;
pub mod remove;
pub mod repeat;
pub mod resume;
pub mod seek;
pub mod shuffle;
pub mod skip;
pub mod spotify;
pub mod stop;
pub mod summon;
pub mod volume;
pub mod vote;
pub mod voteskip;

pub use auditlog::auditlog;
pub use autopause::autopause;
pub use autoplay::autoplay;
pub use clear::clear;
pub use diagnose::diagnose;
pub use doplay::{optplay, play, playfile, playnext, playytplaylist, search};
pub use echoes::echoes;
pub use gambling::{coinflip, roll_n_d, rolldice};
pub use get_metadata::get_metadata;
pub use gp::{gp, GpReveal};
pub use grab::grab;
pub use leave::leave;
pub use lyrics::lyrics;
pub use nowplaying::nowplaying;
pub use np_buttons::buttons;
pub use pause::pause;
pub use playlog::playlog;
pub use queue::queue;
pub use remove::remove;
pub use repeat::repeat;
pub use resume::{resume, resume_internal};
pub use seek::seek;
pub use shuffle::{movesong, shuffle};
pub use skip::{downvote, send_skip_reply, skip};
pub use spotify::spotify;
pub use stop::stop;
pub use summon::summonchannel;
pub use volume::volume;
pub use vote::vote;
pub use voteskip::voteskip;

pub fn music_commands() -> Vec<crate::Command> {
    if cfg!(feature = "crack-music") {
        vec![
            autopause(),
            autoplay(),
            buttons(),
            clear(),
            diagnose(),
            downvote(),
            echoes(),
            ephemeral::ephemeral(),
            grab(),
            leave(),
            lyrics(),
            nowplaying(),
            optplay(),
            pause(),
            play(),
            playfile(),
            playlog(),
            auditlog(),
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
            spotify(),
        ]
    } else {
        vec![]
    }
}

/// Get the game commands. Not registered -- see [`crate::commands::all_commands`].
pub fn game_commands() -> Vec<crate::Command> {
    if cfg!(feature = "crack-music") {
        vec![coinflip(), rolldice()]
    } else {
        vec![]
    }
}

/// The guilty pleasure game. Kept out of [`game_commands`] so that registering
/// it does not drag in `coinflip` and `rolldice`, which are deliberately not
/// registered.
pub fn gp_commands() -> Vec<crate::Command> {
    if cfg!(feature = "crack-music") {
        vec![gp()]
    } else {
        vec![]
    }
}
