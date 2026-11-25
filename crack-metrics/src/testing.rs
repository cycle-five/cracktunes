//! Test support for other crates: capture what a closure records, without naming
//! the `metrics` crate. Enabled by the `testing` cargo feature.
//!
//! The recorder is thread-local ([`metrics::with_local_recorder`]), so the closure
//! must record on the calling thread. For async code under test, drive the future
//! to completion inside the closure on a current-thread runtime, for example
//! `capture(|| rt.block_on(fut))`.

use metrics_util::debugging::{DebugValue, DebuggingRecorder};
use metrics_util::CompositeKey;

type Labels = Vec<(String, String)>;

/// Everything one [`capture`] call recorded.
#[derive(Debug)]
pub struct Captured {
    rows: Vec<(CompositeKey, DebugValue)>,
}

/// Runs `f` with a local debugging recorder and returns everything it recorded.
pub fn capture<R>(f: impl FnOnce() -> R) -> (R, Captured) {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let out = metrics::with_local_recorder(&recorder, f);
    let rows = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .map(|(key, _, _, value)| (key, value))
        .collect();
    (out, Captured { rows })
}

fn labels_of(key: &CompositeKey) -> Labels {
    key.key()
        .labels()
        .map(|l| (l.key().to_owned(), l.value().to_owned()))
        .collect()
}

fn sorted(mut labels: Labels) -> Labels {
    labels.sort();
    labels
}

fn wanted(labels: &[(&str, &str)]) -> Labels {
    sorted(
        labels
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    )
}

impl Captured {
    fn find(&self, name: &str, labels: &[(&str, &str)]) -> Option<&DebugValue> {
        let want = wanted(labels);
        self.rows
            .iter()
            .find(|(k, _)| k.key().name() == name && sorted(labels_of(k)) == want)
            .map(|(_, v)| v)
    }

    /// Counter value for `name` with exactly these labels (order-insensitive); `None` if no such series.
    pub fn counter(&self, name: &str, labels: &[(&str, &str)]) -> Option<u64> {
        match self.find(name, labels) {
            Some(DebugValue::Counter(v)) => Some(*v),
            _ => None,
        }
    }

    /// Gauge value for `name` with exactly these labels; `None` if no such series.
    pub fn gauge(&self, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
        match self.find(name, labels) {
            Some(DebugValue::Gauge(v)) => Some(v.into_inner()),
            _ => None,
        }
    }

    /// Histogram samples for `name` with exactly these labels; empty if no such series.
    pub fn histogram(&self, name: &str, labels: &[(&str, &str)]) -> Vec<f64> {
        match self.find(name, labels) {
            Some(DebugValue::Histogram(v)) => v.iter().map(|x| x.into_inner()).collect(),
            _ => Vec::new(),
        }
    }

    /// Every (name, labels) series recorded, for "nothing else was recorded" assertions.
    pub fn series(&self) -> Vec<(String, Vec<(String, String)>)> {
        self.rows
            .iter()
            .map(|(k, _)| (k.key().name().to_owned(), labels_of(k)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::{counter, gauge, histogram};

    #[test]
    fn reads_back_each_kind_by_name_and_labels() {
        let ((), c) = capture(|| {
            counter!("c", "a" => "1", "b" => "2").increment(3);
            gauge!("g").set(2.5);
            histogram!("h", "x" => "y").record(1.0);
            histogram!("h", "x" => "y").record(2.0);
        });
        assert_eq!(c.counter("c", &[("a", "1"), ("b", "2")]), Some(3));
        assert_eq!(c.gauge("g", &[]), Some(2.5));
        assert_eq!(c.histogram("h", &[("x", "y")]), vec![1.0, 2.0]);
    }

    #[test]
    fn label_order_does_not_matter_but_the_set_must_match_exactly() {
        let ((), c) = capture(|| counter!("c", "a" => "1", "b" => "2").increment(1));
        assert_eq!(c.counter("c", &[("b", "2"), ("a", "1")]), Some(1));
        assert_eq!(c.counter("c", &[("a", "1")]), None);
        assert_eq!(c.counter("c", &[("a", "1"), ("b", "3")]), None);
    }

    #[test]
    fn a_missing_series_or_wrong_kind_reads_as_absent() {
        let ((), c) = capture(|| gauge!("g").set(1.0));
        assert_eq!(c.counter("g", &[]), None);
        assert_eq!(c.gauge("nope", &[]), None);
        assert!(c.histogram("g", &[]).is_empty());
    }

    #[test]
    fn series_lists_everything_recorded_and_returns_the_closure_value() {
        let (n, c) = capture(|| {
            counter!("c", "a" => "1").increment(1);
            gauge!("g").set(1.0);
            7
        });
        assert_eq!(n, 7);
        let mut s = c.series();
        s.sort();
        assert_eq!(
            s,
            vec![
                ("c".to_owned(), vec![("a".to_owned(), "1".to_owned())]),
                ("g".to_owned(), vec![]),
            ]
        );
    }
}
