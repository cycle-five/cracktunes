pub mod admin;
pub mod autopause;
pub mod chatgpt;
pub mod clear;
pub mod collector;
pub mod gambling;
pub mod grab;
pub mod help;
pub mod leave;
pub mod lyrics;
pub mod manage_sources;
pub mod now_playing;
pub mod pause;
pub mod ping;
pub mod play;
pub mod queue;
pub mod remove;
pub mod repeat;
pub mod resume;
pub mod seek;
pub mod shuffle;
pub mod skip;
pub mod stop;
pub mod summon;
pub mod version;
//pub mod test;
pub mod osint;
pub mod volume;
pub mod voteskip;

pub use self::chatgpt::*;
pub use admin::*;
pub use autopause::*;
pub use clear::*;
pub use collector::*;
pub use gambling::*;
pub use grab::*;
pub use help::*;
pub use leave::*;
pub use lyrics::*;
pub use manage_sources::*;
pub use now_playing::*;
pub use pause::*;
pub use ping::*;
pub use play::*;
pub use queue::*;
pub use remove::*;
pub use repeat::*;
pub use resume::*;
pub use seek::*;
pub use shuffle::*;
pub use skip::*;
pub use stop::*;
pub use summon::*;
//pub use test::*;
pub use osint::*;
pub use version::*;
pub use volume::*;
pub use voteskip::*;
