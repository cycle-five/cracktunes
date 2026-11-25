//! What `/play`'s autocomplete suggested, so a suggestion's *label* can be
//! turned back into the link it stood for.
//!
//! A Discord autocomplete choice has a `name` the user sees and a `value` the
//! bot is meant to receive. Ours are `"Title (MM:SS)"` and the track's URL.
//! Discord does not enforce the pairing: the option is free text, and when a
//! picked suggestion is edited or refocused the client submits the label
//! instead. Searching YouTube for `"Tyler Cassidy - Chris Watts (02:53)"`
//! finds nothing -- the duration suffix alone empties the results -- so the
//! song the user chose from our own list failed with "no results".
//!
//! Two layers, tried in order by [`query_from_label`]:
//!
//! 1. [`SuggestionMemory`] remembers recent label -> URL pairs, so a label
//!    that comes back plays exactly the video it was shown for.
//! 2. [`strip_duration_suffix`] drops a trailing `(MM:SS)` from anything else,
//!    for labels suggested before a restart or already evicted.

use crack_metrics::SuggestionLabelOutcome;
use serenity::all::{AutocompleteChoice, AutocompleteValue};
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex, PoisonError};

/// How many suggestions the process remembers. A `/play` autocomplete returns
/// a handful of choices per keystroke, so this covers hundreds of in-progress
/// commands for a few hundred kilobytes.
const CAPACITY: usize = 4096;

/// The suggestions this process has handed to Discord.
static SUGGESTED: LazyLock<SuggestionMemory> =
    LazyLock::new(|| SuggestionMemory::with_capacity(CAPACITY));

/// A bounded label -> URL map that forgets its oldest entry when full.
#[derive(Debug)]
pub struct SuggestionMemory {
    capacity: usize,
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    urls: HashMap<String, String>,
    /// Labels in the order they were first remembered, oldest at the front.
    order: VecDeque<String>,
}

impl SuggestionMemory {
    /// An empty memory holding at most `capacity` suggestions.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Remember that `label` was shown for `url`. A label with nothing to
    /// play behind it is not worth remembering.
    pub fn remember(&self, label: &str, url: &str) {
        let label = label.trim();
        if label.is_empty() || url.is_empty() || self.capacity == 0 {
            return;
        }
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if inner
            .urls
            .insert(label.to_string(), url.to_string())
            .is_some()
        {
            // Already queued for eviction; the newer URL replaces the older.
            return;
        }
        inner.order.push_back(label.to_string());
        while inner.order.len() > self.capacity {
            if let Some(oldest) = inner.order.pop_front() {
                inner.urls.remove(&oldest);
            }
        }
    }

    /// The URL `label` was shown for, if it is still remembered.
    #[must_use]
    pub fn url_for(&self, label: &str) -> Option<String> {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.urls.get(label.trim()).cloned()
    }
}

/// Record the suggestions about to be handed to Discord.
pub fn remember_choices(choices: &[AutocompleteChoice<'_>]) {
    remember_choices_in(&SUGGESTED, choices);
}

fn remember_choices_in(memory: &SuggestionMemory, choices: &[AutocompleteChoice<'_>]) {
    for choice in choices {
        if let AutocompleteValue::String(url) = &choice.value {
            memory.remember(&choice.name, url);
        }
    }
}

/// `query` without a trailing ` (MM:SS)` or ` (HH:MM:SS)`.
///
/// Only a parenthesised run of digits and colons at the very end counts, and
/// only when something is left in front of it: `"(Remastered 2009)"`,
/// `"Song (1999)"` and a bare `"(03:10)"` all come back unchanged.
#[must_use]
pub fn strip_duration_suffix(query: &str) -> &str {
    let trimmed = query.trim_end();
    let Some(body) = trimmed.strip_suffix(')') else {
        return query;
    };
    let Some((title, duration)) = body.rsplit_once('(') else {
        return query;
    };
    let mut parts = duration.split(':');
    let is_duration = matches!(parts.clone().count(), 2 | 3)
        && parts.all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    let title = title.trim_end();
    if is_duration && !title.is_empty() {
        title
    } else {
        query
    }
}

/// What to actually look up for a `/play` query that may be one of our own
/// suggestion labels: the remembered URL, else the query minus its duration.
#[must_use]
pub fn query_from_label(query: &str) -> Cow<'_, str> {
    query_from_label_in(&SUGGESTED, query)
}

fn query_from_label_in<'a>(memory: &SuggestionMemory, query: &'a str) -> Cow<'a, str> {
    if let Some(url) = memory.url_for(query) {
        crack_metrics::suggestion_label(SuggestionLabelOutcome::Remembered);
        return Cow::Owned(url);
    }
    let stripped = strip_duration_suffix(query);
    if stripped != query {
        crack_metrics::suggestion_label(SuggestionLabelOutcome::Stripped);
    }
    Cow::Borrowed(stripped)
}

#[cfg(test)]
mod tests {
    use super::{
        query_from_label_in, remember_choices_in, strip_duration_suffix, SuggestionMemory,
    };
    use serenity::all::{AutocompleteChoice, AutocompleteValue};
    use std::borrow::Cow;

    const LABEL: &str = "Tyler Cassidy - Chris Watts (02:53)";
    const URL: &str = "https://www.youtube.com/watch?v=ZijVdAp3PVA";

    #[test]
    fn strips_a_trailing_duration() {
        // The production failure of 2026-11-25: this exact string had no results.
        assert_eq!(strip_duration_suffix(LABEL), "Tyler Cassidy - Chris Watts");
        assert_eq!(strip_duration_suffix("Long Mix (01:02:03)"), "Long Mix");
        assert_eq!(strip_duration_suffix("Song (2:53)  "), "Song");
        assert_eq!(strip_duration_suffix("Song(02:53)"), "Song");
    }

    #[test]
    fn leaves_everything_else_alone() {
        for query in [
            "tyler cassidy chris watts",
            "Wait (Remastered 2009)",
            "Song (1999)",
            "Song (02:53) live",
            "Song (1:2:3:4)",
            "Song (:53)",
            "Song (02:)",
            "Song (02:5a)",
            "Song 02:53",
            "(02:53)",
            "  (02:53)",
            "Live Stream (∞)",
            "https://www.youtube.com/watch?v=ZijVdAp3PVA",
            "",
        ] {
            assert_eq!(strip_duration_suffix(query), query, "{query:?}");
        }
    }

    #[test]
    fn a_remembered_label_becomes_its_url() {
        let memory = SuggestionMemory::with_capacity(8);
        memory.remember(LABEL, URL);
        assert_eq!(query_from_label_in(&memory, LABEL), URL);
        // Discord may hand the text back with stray whitespace.
        assert_eq!(query_from_label_in(&memory, &format!(" {LABEL} ")), URL);
    }

    #[test]
    fn the_choices_we_send_are_what_gets_remembered() {
        let memory = SuggestionMemory::with_capacity(8);
        let choices = [
            AutocompleteChoice {
                name: Cow::Borrowed(LABEL),
                value: AutocompleteValue::String(Cow::Borrowed(URL)),
                name_localizations: None,
            },
            AutocompleteChoice {
                name: Cow::Borrowed("Seven"),
                value: AutocompleteValue::Integer(7),
                name_localizations: None,
            },
        ];
        remember_choices_in(&memory, &choices);
        assert_eq!(query_from_label_in(&memory, LABEL), URL);
        assert_eq!(memory.url_for("Seven"), None);
    }

    #[test]
    fn an_unknown_label_loses_its_duration() {
        let memory = SuggestionMemory::with_capacity(8);
        memory.remember("Some Other Song (03:10)", URL);
        assert_eq!(
            query_from_label_in(&memory, LABEL),
            "Tyler Cassidy - Chris Watts"
        );
        assert_eq!(query_from_label_in(&memory, "plain words"), "plain words");
    }

    #[test]
    fn a_label_truncated_to_no_duration_is_still_remembered() {
        // `suggest_string` cuts long titles before the duration, so the strip
        // cannot help these; only the memory can.
        let label = "An Extremely Long Title That Discord Cut Off Somewhere In The Mid";
        let memory = SuggestionMemory::with_capacity(8);
        memory.remember(label, URL);
        assert_eq!(query_from_label_in(&memory, label), URL);
    }

    #[test]
    fn forgets_the_oldest_when_full() {
        let memory = SuggestionMemory::with_capacity(2);
        memory.remember("a", "url-a");
        memory.remember("b", "url-b");
        memory.remember("c", "url-c");
        assert_eq!(memory.url_for("a"), None);
        assert_eq!(memory.url_for("b").as_deref(), Some("url-b"));
        assert_eq!(memory.url_for("c").as_deref(), Some("url-c"));
    }

    #[test]
    fn remembering_again_updates_without_growing() {
        let memory = SuggestionMemory::with_capacity(2);
        memory.remember("a", "url-a");
        memory.remember("a", "url-a2");
        memory.remember("b", "url-b");
        // Had the repeat taken a second slot, "a" would have been evicted here.
        assert_eq!(memory.url_for("a").as_deref(), Some("url-a2"));
        assert_eq!(memory.url_for("b").as_deref(), Some("url-b"));
        memory.remember("c", "url-c");
        assert_eq!(memory.url_for("a"), None);
        assert_eq!(memory.url_for("c").as_deref(), Some("url-c"));
    }

    #[test]
    fn does_not_remember_a_label_with_nothing_behind_it() {
        let memory = SuggestionMemory::with_capacity(2);
        memory.remember(LABEL, "");
        memory.remember("", URL);
        assert_eq!(memory.url_for(LABEL), None);
        assert_eq!(memory.url_for(""), None);
        // And neither took a slot.
        memory.remember("a", "url-a");
        memory.remember("b", "url-b");
        assert_eq!(memory.url_for("a").as_deref(), Some("url-a"));
    }

    fn series_of(query: &str, remember: bool) -> Vec<(String, Vec<(String, String)>)> {
        let memory = SuggestionMemory::with_capacity(8);
        if remember {
            memory.remember(query, URL);
        }
        crack_metrics::testing::capture(|| {
            let _ = query_from_label_in(&memory, query);
        })
        .1
        .series()
    }

    #[test]
    fn a_remembered_label_records_remembered() {
        let c = crack_metrics::testing::capture(|| {
            let memory = SuggestionMemory::with_capacity(8);
            memory.remember(LABEL, URL);
            let _ = query_from_label_in(&memory, LABEL);
        })
        .1;
        assert_eq!(
            c.counter(
                crack_metrics::names::SUGGESTION_LABEL,
                &[("outcome", "remembered")]
            ),
            Some(1)
        );
        assert_eq!(c.series().len(), 1);
    }

    #[test]
    fn a_stripped_label_records_stripped() {
        let c = crack_metrics::testing::capture(|| {
            let memory = SuggestionMemory::with_capacity(8);
            let _ = query_from_label_in(&memory, LABEL);
        })
        .1;
        assert_eq!(
            c.counter(
                crack_metrics::names::SUGGESTION_LABEL,
                &[("outcome", "stripped")]
            ),
            Some(1)
        );
        assert_eq!(c.series().len(), 1);
    }

    #[test]
    fn a_plain_query_records_nothing() {
        assert!(series_of("tyler cassidy chris watts", false).is_empty());
    }
}
