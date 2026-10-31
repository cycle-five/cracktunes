use fakecord::fakechars::FixedStr;

pub fn label() -> FixedStr {
    FixedStr("x".into())
}

pub fn label_inline() -> String {
    fakecord::fakechars::FixedStr("y".into()).0
}

pub const BANNER: &str = "🎵🎵"; pub fn after_emoji() -> u32 { 7 }

pub fn emoji_user() -> u32 {
    after_emoji()
}
