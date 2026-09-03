//! Prompt categories for the `/gp` game.
//!
//! The prompts live in `gp_prompts.json` next to this file and are compiled in
//! with `include_str!`: unlike the `./data/*.json` files, nothing has to exist
//! on disk at runtime (the Docker image puts `data/` at `/data` while the bot
//! runs from `/app`, so a runtime read would silently miss there).

use once_cell::sync::Lazy;
use poise::ChoiceParameter;
use rand::{seq::SliceRandom, Rng};
use serde::Deserialize;

/// The category the host picks when starting a game. The first `#[name]` is
/// what Discord shows in the slash-command dropdown; the second is the short
/// alias prefix commands can type (`!gp start nostalgia 5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ChoiceParameter)]
pub enum GpCategory {
    #[name = "🥹 Nostalgia"]
    #[name = "nostalgia"]
    Nostalgia,
    #[name = "🔥 Slightly More Dangerous"]
    #[name = "dangerous"]
    Dangerous,
    #[name = "🎶 The Really Good Game Prompts"]
    #[name = "good_game"]
    GoodGame,
    #[name = "🚗 Car / Driving"]
    #[name = "car"]
    Car,
    #[name = "🌿 Altered-State / Chill"]
    #[name = "chill"]
    Chill,
    #[name = "😭 Emotional"]
    #[name = "emotional"]
    Emotional,
    #[name = "🤢 Bad Music"]
    #[name = "bad_music"]
    BadMusic,
    #[name = "🎧 Hyper-Specific"]
    #[name = "hyper_specific"]
    HyperSpecific,
    #[name = "🖤 Weirdly Revealing"]
    #[name = "revealing"]
    Revealing,
    #[name = "😂 Game Chaos"]
    #[name = "chaos"]
    Chaos,
    #[name = "⚡ One-Worders"]
    #[name = "one_worders"]
    OneWorders,
    #[name = "🎤 Social / Go-To"]
    #[name = "social"]
    Social,
    #[name = "😈 Guilty Pleasures / Secret Taste"]
    #[name = "guilty"]
    Guilty,
    #[name = "💋 Sex / Romance / Attraction"]
    #[name = "romance"]
    Romance,
    #[name = "🥀 Emotional Damage"]
    #[name = "damage"]
    Damage,
    #[name = "🕺 Chaotic / Funny"]
    #[name = "funny"]
    Funny,
    #[name = "🧠 Personality Reveals"]
    #[name = "personality"]
    Personality,
    /// Draws from every category.
    #[name = "🎲 Mixed"]
    #[name = "mixed"]
    Mixed,
}

impl GpCategory {
    /// The `key` of the matching entry in `gp_prompts.json`; `None` for Mixed.
    pub fn key(self) -> Option<&'static str> {
        Some(match self {
            Self::Nostalgia => "nostalgia",
            Self::Dangerous => "dangerous",
            Self::GoodGame => "good_game",
            Self::Car => "car",
            Self::Chill => "chill",
            Self::Emotional => "emotional",
            Self::BadMusic => "bad_music",
            Self::HyperSpecific => "hyper_specific",
            Self::Revealing => "revealing",
            Self::Chaos => "chaos",
            Self::OneWorders => "one_worders",
            Self::Social => "social",
            Self::Guilty => "guilty",
            Self::Romance => "romance",
            Self::Damage => "damage",
            Self::Funny => "funny",
            Self::Personality => "personality",
            Self::Mixed => return None,
        })
    }

    /// The display name (with emoji), as Discord shows it.
    pub fn display(self) -> &'static str {
        self.name()
    }

    /// Every prompt this category can draw from.
    pub fn pool(self) -> Vec<&'static str> {
        match self.key() {
            Some(key) => GP_PROMPTS
                .iter()
                .filter(|c| c.key == key)
                .flat_map(|c| c.prompts.iter().map(String::as_str))
                .collect(),
            None => GP_PROMPTS
                .iter()
                .flat_map(|c| c.prompts.iter().map(String::as_str))
                .collect(),
        }
    }
}

/// One category as stored in `gp_prompts.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct GpPromptCategory {
    pub key: String,
    pub name: String,
    pub prompts: Vec<String>,
}

/// The bundled prompt data, parsed once.
pub static GP_PROMPTS: Lazy<Vec<GpPromptCategory>> = Lazy::new(|| {
    serde_json::from_str(include_str!("gp_prompts.json")).expect("gp_prompts.json is valid")
});

/// Draw up to `n` distinct prompts from `category`, in random order. Returns
/// fewer than `n` when the category is smaller than that.
pub fn draw_prompts(category: GpCategory, n: usize, rng: &mut impl Rng) -> Vec<String> {
    let pool = category.pool();
    let n = n.min(pool.len());
    let mut picked: Vec<String> = pool
        .choose_multiple(rng, n)
        .map(|s| s.to_string())
        .collect();
    picked.shuffle(rng);
    picked
}

#[cfg(test)]
mod test {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};
    use std::collections::HashSet;

    #[test]
    fn prompt_data_is_valid() {
        let cats = &*GP_PROMPTS;
        assert!(!cats.is_empty());
        let keys: HashSet<&str> = cats.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys.len(), cats.len(), "duplicate category keys");
        for c in cats {
            assert!(!c.prompts.is_empty(), "{} has no prompts", c.key);
            assert!(
                c.prompts
                    .iter()
                    .all(|p| !p.trim().is_empty() && p.len() < 200),
                "{} has an empty or overlong prompt",
                c.key
            );
            let distinct: HashSet<&str> = c.prompts.iter().map(String::as_str).collect();
            assert_eq!(
                distinct.len(),
                c.prompts.len(),
                "{} repeats a prompt",
                c.key
            );
        }
        // Every JSON category is a choice, plus Mixed; names match what Discord shows.
        let choices = GpCategory::list();
        assert_eq!(choices.len(), cats.len() + 1, "enum and JSON disagree");
        assert!(choices.len() <= 25, "Discord caps choices at 25");
        for c in cats {
            let variant = choices
                .iter()
                .find(|ch| ch.name == c.name)
                .unwrap_or_else(|| panic!("no choice named {}", c.name));
            let parsed = GpCategory::from_name(&variant.name).expect("from_name(display)");
            assert_eq!(parsed.key(), Some(c.key.as_str()));
            assert_eq!(
                GpCategory::from_name(&c.key),
                Some(parsed),
                "alias {}",
                c.key
            );
        }
        assert_eq!(GpCategory::from_name("mixed"), Some(GpCategory::Mixed));
        assert_eq!(
            GpCategory::from_name("NOSTALGIA"),
            Some(GpCategory::Nostalgia)
        );
        assert_eq!(GpCategory::from_name("nope"), None);
        assert_eq!(GpCategory::Mixed.key(), None);
    }

    #[test]
    fn pools() {
        let total: usize = GP_PROMPTS.iter().map(|c| c.prompts.len()).sum();
        assert_eq!(GpCategory::Mixed.pool().len(), total);
        assert_eq!(GpCategory::Car.pool().len(), 7);
        assert!(GpCategory::Car
            .pool()
            .contains(&"What song do you blast with the windows down?"));
        assert_eq!(GpCategory::Nostalgia.display(), "🥹 Nostalgia");
    }

    #[test]
    fn draw_prompts_no_repeats_and_capped() {
        let mut rng = StdRng::seed_from_u64(7);
        let drawn = draw_prompts(GpCategory::Emotional, 5, &mut rng);
        assert_eq!(drawn.len(), 5);
        let distinct: HashSet<&String> = drawn.iter().collect();
        assert_eq!(distinct.len(), 5);
        let pool = GpCategory::Emotional.pool();
        assert!(drawn.iter().all(|p| pool.contains(&p.as_str())));

        // More rounds than prompts: capped at the pool size, still distinct.
        let drawn = draw_prompts(GpCategory::Car, 20, &mut rng);
        assert_eq!(drawn.len(), 7);
        let distinct: HashSet<&String> = drawn.iter().collect();
        assert_eq!(distinct.len(), 7);

        // Mixed draws across categories.
        let drawn = draw_prompts(GpCategory::Mixed, 20, &mut rng);
        assert_eq!(drawn.len(), 20);
        let distinct: HashSet<&String> = drawn.iter().collect();
        assert_eq!(distinct.len(), 20);

        // Seeded draws are reproducible.
        let a = draw_prompts(GpCategory::Chaos, 4, &mut StdRng::seed_from_u64(1));
        let b = draw_prompts(GpCategory::Chaos, 4, &mut StdRng::seed_from_u64(1));
        assert_eq!(a, b);
    }
}
