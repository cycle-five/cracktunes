//! Platform-neutral types for Crack Tunes. Depends on no chat-platform crate.

pub mod ids;
pub use ids::{
    Community, CommunityRef, Kind, Message, Platform, Ref, Role, RoleRef, Room, RoomRef, User,
    UserRef,
};

pub mod output;
pub use output::{
    colour, Author, Button, ButtonStyle, Card, Field, Footer, Mention, Mentions, Rendered, Row,
    Select, SelectOption,
};
