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

/// What the host picks in `/gp start`'s category dropdown. The first `#[name]`
/// is what Discord shows; the others are what prefix commands can type
/// (`!gp start nostalgia 5`). The first two are not categories: Random is all
/// of them, and Pick several is answered with a menu to tick some. A game
/// holds the outcome as [`GpCategories`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChoiceParameter)]
pub enum GpCategory {
    /// First, so it tops the dropdown; also what `/gp start` plays when no
    /// category is given. `mixed` is the name it had before.
    #[name = "🎲 Random"]
    #[name = "random"]
    #[name = "mixed"]
    Random,
    #[name = "☑️ Pick several…"]
    #[name = "pick"]
    PickSeveral,
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
}

impl GpCategory {
    /// The categories that hold prompts, in dropdown order.
    pub const CATEGORIES: [GpCategory; 17] = [
        Self::Nostalgia,
        Self::Dangerous,
        Self::GoodGame,
        Self::Car,
        Self::Chill,
        Self::Emotional,
        Self::BadMusic,
        Self::HyperSpecific,
        Self::Revealing,
        Self::Chaos,
        Self::OneWorders,
        Self::Social,
        Self::Guilty,
        Self::Romance,
        Self::Damage,
        Self::Funny,
        Self::Personality,
    ];

    /// The `key` of the matching entry in `gp_prompts.json`, which is also how
    /// the category is stored. `None` for Random and Pick several.
    pub fn key(self) -> Option<&'static str> {
        Some(match self {
            Self::Random | Self::PickSeveral => return None,
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
        })
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::CATEGORIES.into_iter().find(|c| c.key() == Some(key))
    }

    /// The display name (with emoji), as Discord shows it.
    pub fn display(self) -> &'static str {
        self.name()
    }

    /// Every prompt this category holds; none for Random and Pick several.
    pub fn pool(self) -> Vec<&'static str> {
        let Some(key) = self.key() else {
            return Vec::new();
        };
        GP_PROMPTS
            .iter()
            .filter(|c| c.key == key)
            .flat_map(|c| c.prompts.iter().map(String::as_str))
            .collect()
    }
}

/// The categories a game draws its prompts from: one, several, or all of them
/// (Random). Never empty, holds only categories with prompts, and keeps them in
/// dropdown order, so the same pick always compares and stores the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpCategories(Vec<GpCategory>);

/// How a game drawing from every category is stored: what Random was saved as
/// back when it was called Mixed.
const GP_ALL_SLUG: &str = "mixed";

impl GpCategories {
    pub fn all() -> Self {
        Self(GpCategory::CATEGORIES.to_vec())
    }

    /// What one dropdown choice plays. `None` for Pick several, which is
    /// answered with a menu instead.
    pub fn from_choice(choice: GpCategory) -> Option<Self> {
        match choice {
            GpCategory::PickSeveral => None,
            c => Self::new([c]),
        }
    }

    /// The categories picked, in any order and however often. Random among
    /// them is all of them. `None` when nothing with prompts was picked.
    pub fn new(picked: impl IntoIterator<Item = GpCategory>) -> Option<Self> {
        let picked: Vec<GpCategory> = picked.into_iter().collect();
        if picked.contains(&GpCategory::Random) {
            return Some(Self::all());
        }
        let set: Vec<GpCategory> = GpCategory::CATEGORIES
            .into_iter()
            .filter(|c| picked.contains(c))
            .collect();
        (!set.is_empty()).then_some(Self(set))
    }

    pub fn as_slice(&self) -> &[GpCategory] {
        &self.0
    }

    pub fn is_all(&self) -> bool {
        self.0.len() == GpCategory::CATEGORIES.len()
    }

    /// For `gp_game.category`: the keys joined by commas, so a game saved when
    /// a game had one category reads back as a set of one; `mixed` for all.
    pub fn slug(&self) -> String {
        if self.is_all() {
            return GP_ALL_SLUG.to_string();
        }
        self.0
            .iter()
            .filter_map(|c| c.key())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Refuses anything [`Self::slug`] would not write, rather than dropping
    /// the part it does not know.
    pub fn from_slug(slug: &str) -> Option<Self> {
        if slug == GP_ALL_SLUG {
            return Some(Self::all());
        }
        let picked = slug
            .split(',')
            .map(GpCategory::from_key)
            .collect::<Option<Vec<_>>>()?;
        Self::new(picked)
    }

    /// For the start message: `🎲 Random` for all of them, otherwise the names.
    pub fn display(&self) -> String {
        if self.is_all() {
            return GpCategory::Random.display().to_string();
        }
        self.0
            .iter()
            .map(|c| c.display())
            .collect::<Vec<_>>()
            .join(", ")
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

/// A drawn prompt and the category it came from, which in a game of several
/// categories changes from round to round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpPrompt {
    pub category: GpCategory,
    pub text: String,
}

/// Draw up to `n` distinct prompts from `categories`, in random order. Returns
/// fewer than `n` when there are fewer prompts than that.
///
/// Each round draws a category and then a prompt from it, so every category is
/// as likely as any other however many prompts it holds -- drawing from all
/// the prompts at once gives One-Worders nearly three times Car's share. Every
/// category comes up once before any comes up again, and none twice in a row.
pub fn draw_prompts(categories: &GpCategories, n: usize, rng: &mut impl Rng) -> Vec<GpPrompt> {
    let mut pools: Vec<(GpCategory, Vec<&'static str>)> = categories
        .as_slice()
        .iter()
        .map(|c| {
            let mut pool = c.pool();
            pool.shuffle(rng);
            (*c, pool)
        })
        .collect();
    let mut drawn: Vec<GpPrompt> = Vec::with_capacity(n);
    while drawn.len() < n {
        pools.retain(|(_, pool)| !pool.is_empty());
        if pools.is_empty() {
            break;
        }
        // One pass is every category left, in a fresh order; the category that
        // ended the last pass must not start this one.
        pools.shuffle(rng);
        if let Some(last) = drawn.last() {
            if pools.len() > 1 && pools[0].0 == last.category {
                let end = pools.len() - 1;
                pools.swap(0, end);
            }
        }
        for (c, pool) in pools.iter_mut() {
            if drawn.len() == n {
                break;
            }
            if let Some(text) = pool.pop() {
                drawn.push(GpPrompt {
                    category: *c,
                    text: text.to_string(),
                });
            }
        }
    }
    drawn
}

#[cfg(test)]
mod test {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};
    use std::collections::{HashMap, HashSet};

    fn set(cs: &[GpCategory]) -> GpCategories {
        GpCategories::new(cs.iter().copied()).unwrap()
    }

    #[test]
    fn a_set_of_categories_round_trips_through_its_slug() {
        for c in GpCategory::CATEGORIES {
            let one = GpCategories::from_choice(c).unwrap();
            assert_eq!(one.as_slice(), &[c]);
            assert_eq!(one.slug(), c.key().unwrap());
            assert_eq!(GpCategories::from_slug(&one.slug()), Some(one), "{c:?}");
        }
        // Order and repeats in the pick do not matter.
        let picked = set(&[GpCategory::Chill, GpCategory::Car, GpCategory::Chill]);
        assert_eq!(picked.as_slice(), &[GpCategory::Car, GpCategory::Chill]);
        assert_eq!(picked.slug(), "car,chill");
        assert_eq!(GpCategories::from_slug("chill,car"), Some(picked.clone()));
        assert_eq!(
            picked.display(),
            "🚗 Car / Driving, 🌿 Altered-State / Chill"
        );

        // Random is every category, stored the way Mixed was.
        let all = GpCategories::from_choice(GpCategory::Random).unwrap();
        assert!(all.is_all());
        assert_eq!(all, GpCategories::all());
        assert_eq!(all.slug(), "mixed");
        assert_eq!(GpCategories::from_slug("mixed"), Some(all.clone()));
        assert_eq!(all.display(), "🎲 Random");
        assert_eq!(GpCategories::new(GpCategory::CATEGORIES), Some(all.clone()));
        assert_eq!(
            GpCategories::new([GpCategory::Car, GpCategory::Random]),
            Some(all)
        );

        assert_eq!(GpCategories::from_choice(GpCategory::PickSeveral), None);
        assert_eq!(GpCategories::new([]), None);
        assert_eq!(GpCategories::new([GpCategory::PickSeveral]), None);
        for bad in ["", "🎲 Random", "random", "car,", "car,nope", "car,mixed"] {
            assert_eq!(GpCategories::from_slug(bad), None, "{bad:?}");
        }
    }

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
        // Nor across categories, or a game of several could ask the same thing twice.
        let every: Vec<&str> = cats
            .iter()
            .flat_map(|c| c.prompts.iter().map(String::as_str))
            .collect();
        let distinct: HashSet<&str> = every.iter().copied().collect();
        assert_eq!(distinct.len(), every.len(), "a prompt is in two categories");

        // Every JSON category is a choice, after Random and Pick several; names
        // match what Discord shows.
        let choices = GpCategory::list();
        assert_eq!(choices.len(), cats.len() + 2, "enum and JSON disagree");
        assert!(choices.len() <= 25, "Discord caps choices at 25");
        assert_eq!(choices[0].name, "🎲 Random", "Random tops the dropdown");
        assert_eq!(choices[1].name, "☑️ Pick several…");
        assert_eq!(GpCategory::CATEGORIES.len(), cats.len());
        for (c, choice) in cats.iter().zip(&choices[2..]) {
            assert_eq!(choice.name, c.name, "dropdown order follows the JSON");
            let parsed = GpCategory::from_name(&choice.name).expect("from_name(display)");
            assert_eq!(parsed.key(), Some(c.key.as_str()));
            assert_eq!(GpCategory::from_key(&c.key), Some(parsed));
            assert_eq!(
                GpCategory::from_name(&c.key),
                Some(parsed),
                "alias {}",
                c.key
            );
        }
        for name in ["🎲 Random", "random", "RANDOM", "mixed"] {
            assert_eq!(
                GpCategory::from_name(name),
                Some(GpCategory::Random),
                "{name}"
            );
        }
        assert_eq!(GpCategory::from_name("pick"), Some(GpCategory::PickSeveral));
        assert_eq!(
            GpCategory::from_name("NOSTALGIA"),
            Some(GpCategory::Nostalgia)
        );
        assert_eq!(GpCategory::from_name("nope"), None);
        assert_eq!(GpCategory::Random.key(), None);
        assert_eq!(GpCategory::PickSeveral.key(), None);
        assert_eq!(GpCategory::from_key("mixed"), None);
    }

    #[test]
    fn pools() {
        assert_eq!(GpCategory::Car.pool().len(), 7);
        assert!(GpCategory::Car
            .pool()
            .contains(&"What song do you blast with the windows down?"));
        assert!(GpCategory::Random.pool().is_empty());
        assert!(GpCategory::PickSeveral.pool().is_empty());
        assert_eq!(GpCategory::Nostalgia.display(), "🥹 Nostalgia");
    }

    fn texts(drawn: &[GpPrompt]) -> HashSet<&str> {
        drawn.iter().map(|p| p.text.as_str()).collect()
    }

    #[test]
    fn one_category_draws_distinct_prompts_capped_at_its_size() {
        let mut rng = StdRng::seed_from_u64(7);
        let emotional = set(&[GpCategory::Emotional]);
        let drawn = draw_prompts(&emotional, 5, &mut rng);
        assert_eq!(drawn.len(), 5);
        assert_eq!(texts(&drawn).len(), 5);
        let pool = GpCategory::Emotional.pool();
        assert!(drawn.iter().all(|p| pool.contains(&p.text.as_str())));
        assert!(drawn.iter().all(|p| p.category == GpCategory::Emotional));

        // More rounds than prompts: capped at the pool size, still distinct.
        let drawn = draw_prompts(&set(&[GpCategory::Car]), 20, &mut rng);
        assert_eq!(drawn.len(), 7);
        assert_eq!(texts(&drawn).len(), 7);

        // Seeded draws are reproducible.
        let chaos = set(&[GpCategory::Chaos]);
        let a = draw_prompts(&chaos, 4, &mut StdRng::seed_from_u64(1));
        let b = draw_prompts(&chaos, 4, &mut StdRng::seed_from_u64(1));
        assert_eq!(a, b);
    }

    /// Checks a draw from `categories` against everything [`draw_prompts`]
    /// promises.
    fn check_draw(categories: &GpCategories, n: usize, seed: u64) {
        let drawn = draw_prompts(categories, n, &mut StdRng::seed_from_u64(seed));
        let total: usize = categories.as_slice().iter().map(|c| c.pool().len()).sum();
        assert_eq!(drawn.len(), n.min(total));
        assert_eq!(
            texts(&drawn).len(),
            drawn.len(),
            "seed {seed}: a prompt repeated"
        );
        for p in &drawn {
            assert!(categories.as_slice().contains(&p.category));
            assert!(
                p.category.pool().contains(&p.text.as_str()),
                "seed {seed}: {:?} is not from {:?}",
                p.text,
                p.category
            );
        }
        // Within a pass no category repeats; across passes none back to back.
        let width = categories.as_slice().len();
        for pass in drawn.chunks(width) {
            let cats: HashSet<GpCategory> = pass.iter().map(|p| p.category).collect();
            assert_eq!(cats.len(), pass.len(), "seed {seed}: a category repeated");
        }
        if width > 1 {
            assert!(
                drawn.windows(2).all(|w| w[0].category != w[1].category),
                "seed {seed}: the same category twice in a row"
            );
        }
    }

    #[test]
    fn several_categories_take_turns() {
        let three = set(&[GpCategory::Car, GpCategory::Chill, GpCategory::Nostalgia]);
        let all = GpCategories::all();
        for seed in 0..200 {
            check_draw(&all, GpCategory::CATEGORIES.len(), seed);
            check_draw(&all, 20, seed);
            check_draw(&all, 5, seed);
            check_draw(&three, 3, seed);
            check_draw(&three, 12, seed);
        }

        // Asked for more than there is: Car and Chill run out after seven each,
        // Nostalgia carries on alone, and every prompt of the three comes up once.
        let drawn = draw_prompts(&three, 30, &mut StdRng::seed_from_u64(3));
        assert_eq!(drawn.len(), 24);
        assert_eq!(texts(&drawn).len(), 24);
        let n = |c| drawn.iter().filter(|p| p.category == c).count();
        assert_eq!(
            (
                n(GpCategory::Car),
                n(GpCategory::Chill),
                n(GpCategory::Nostalgia)
            ),
            (7, 7, 10)
        );

        let a = draw_prompts(&all, 5, &mut StdRng::seed_from_u64(1));
        let b = draw_prompts(&all, 5, &mut StdRng::seed_from_u64(1));
        assert_eq!(a, b);
    }

    #[test]
    fn random_favours_no_category_however_many_prompts_it_has() {
        // One-Worders has 20 prompts and Car 7; by prompt count One-Worders
        // would open a game almost three times as often.
        let draws = 17_000;
        let all = GpCategories::all();
        let mut rng = StdRng::seed_from_u64(11);
        let mut firsts: HashMap<GpCategory, usize> = HashMap::new();
        for _ in 0..draws {
            let first = draw_prompts(&all, 1, &mut rng)[0].category;
            *firsts.entry(first).or_insert(0) += 1;
        }
        let expected = draws / GpCategory::CATEGORIES.len();
        assert_eq!(firsts.len(), GpCategory::CATEGORIES.len());
        for (c, n) in firsts {
            assert!(
                n.abs_diff(expected) < expected / 5,
                "{c:?} opened {n} of {draws} games, expected about {expected}"
            );
        }
    }
}
