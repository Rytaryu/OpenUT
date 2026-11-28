//! Comparison of saved replay expectations with observed output.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write;

use openut_core::time::{LogDate, LogTime, UtcOffset};
use serde::Deserialize;
use toml::{Table, Value, value::Datetime};

#[derive(Debug, Deserialize)]
pub struct Expectations {
    pub check_rules: Vec<String>,
    #[serde(default)]
    pub replay: ReplayOptions,
    #[serde(default)]
    pub rules: Table,
    #[serde(default, rename = "detection")]
    detections: Vec<ExpectedDetection>,
    #[serde(default)]
    summary: BTreeMap<String, BTreeMap<String, Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ReplayOptions {
    pub flush_s: u32,
    pub catch_up: bool,
}

impl Default for ReplayOptions {
    fn default() -> Self {
        Self {
            flush_s: 600,
            catch_up: false,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ExpectedDetection {
    rule: String,
    tracker: Option<u16>,
    after: LocalTime,
    before: LocalTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
struct LocalTime(i64);

impl TryFrom<String> for LocalTime {
    type Error = String;

    fn try_from(source: String) -> Result<Self, Self::Error> {
        let invalid_time = || format!("地方時を読み取れませんでした（{source}）");
        let parsed: Datetime = source.parse().map_err(|_| invalid_time())?;
        let (Some(date), Some(time), None) = (parsed.date, parsed.time, parsed.offset) else {
            return Err(invalid_time());
        };
        let nanoseconds = time.nanosecond.unwrap_or(0);
        // Observations have millisecond precision. Round the boundary up so both
        // inclusive lower and exclusive upper bounds also work within a millisecond.
        let base = LogTime::from_local(
            LogDate {
                year: date.year.into(),
                month: date.month,
                day: date.day,
            },
            time.hour,
            time.minute,
            time.second.unwrap_or(0),
            0,
            UtcOffset(0),
        )
        .ok_or_else(invalid_time)?;
        Ok(Self(base.0 + i64::from(nanoseconds.div_ceil(1_000_000))))
    }
}

/// A `Raised` detection from the replay engine with the log's offset at that instant.
#[derive(Debug)]
pub struct RaisedDetection<'a> {
    pub rule: &'a str,
    pub tracker: Option<u16>,
    pub at: LogTime,
    pub offset: UtcOffset,
}

impl RaisedDetection<'_> {
    fn local_time(&self) -> LocalTime {
        LocalTime(self.at.0.saturating_add(i64::from(self.offset.0) * 60_000))
    }
}

/// Observed summary tables by local calendar day, using the nested §8 field names.
pub type ObservedSummaries = BTreeMap<String, Table>;

impl Expectations {
    pub fn parse(source: &str) -> Result<Self, String> {
        let expectations: Self = toml::from_str(source).map_err(|error| error.to_string())?;
        for expected in &expectations.detections {
            if !expectations.check_rules.contains(&expected.rule) {
                return Err(format!("check_rules に規則 {} がありません", expected.rule));
            }
            if expected.before <= expected.after {
                return Err(format!(
                    "規則 {} の after は before より前にしてください",
                    expected.rule
                ));
            }
        }
        Ok(expectations)
    }

    /// Pair expectations and observations chronologically, reassigning a match when needed.
    pub fn compare(
        &self,
        detections: &[RaisedDetection<'_>],
        summaries: &ObservedSummaries,
    ) -> Result<(), String> {
        let mut expected: Vec<_> = self.detections.iter().collect();
        expected.sort_by_key(|entry| {
            (
                entry.after,
                entry.rule.as_str(),
                entry.tracker,
                entry.before,
            )
        });
        let mut actual: Vec<_> = detections.iter().collect();
        actual.sort_by_key(|entry| (entry.at, entry.rule, entry.tracker, entry.offset.0));

        let mut report = String::new();
        let mut matched = vec![None; actual.len()];
        for (index, entry) in expected.iter().enumerate() {
            if !match_detection(index, &expected, &actual, &mut matched) {
                let _ = writeln!(report, "見つからない検出　{}", describe_expected(entry));
            }
        }
        for (entry, expected_index) in actual.iter().zip(matched) {
            if expected_index.is_none() && self.check_rules.iter().any(|rule| rule == entry.rule) {
                let _ = writeln!(report, "余分な検出　{}", describe_observation(entry));
            }
        }
        for (day, fields) in &self.summary {
            for (path, expected) in fields {
                let actual = summaries
                    .get(day)
                    .and_then(|summary| summary_value(summary, path));
                if actual != Some(expected) {
                    let actual = actual.map_or_else(|| "なし".to_owned(), ToString::to_string);
                    let _ = writeln!(report, "集計 {day} {path}　期待 {expected}　実際 {actual}");
                }
            }
        }

        if report.is_empty() {
            return Ok(());
        }
        let _ = writeln!(report, "期待する検出");
        for entry in expected {
            let _ = writeln!(report, "  {}", describe_expected(entry));
        }
        let _ = writeln!(report, "実際の検出");
        for entry in actual {
            let _ = writeln!(report, "  {}", describe_observation(entry));
        }
        Err(report)
    }
}

fn match_detection(
    requested: usize,
    expected: &[&ExpectedDetection],
    actual: &[&RaisedDetection<'_>],
    matched: &mut [Option<usize>],
) -> bool {
    // Search in chronological order for an unused observation. If all compatible
    // observations are taken, look for another observation for one of their expectations.
    let mut pending = VecDeque::from([(requested, None)]);
    let mut previous = vec![None; actual.len()];
    while let Some((expected_index, displaced)) = pending.pop_front() {
        let entry = expected[expected_index];
        for (index, observation) in actual.iter().enumerate() {
            if previous[index].is_some() || !matches_detection(entry, observation) {
                continue;
            }
            previous[index] = Some((expected_index, displaced));
            if let Some(owner) = matched[index] {
                pending.push_back((owner, Some(index)));
                continue;
            }

            // An unused observation completes the chain. Keep all the displaced
            // expectations matched by moving them along this chain.
            let mut current = Some(index);
            while let Some(index) = current {
                let Some((owner, displaced)) = previous[index] else {
                    break;
                };
                matched[index] = Some(owner);
                current = displaced;
            }
            return true;
        }
    }
    false
}

fn matches_detection(entry: &ExpectedDetection, observation: &RaisedDetection<'_>) -> bool {
    entry.rule == observation.rule
        && entry
            .tracker
            .is_none_or(|tracker| observation.tracker == Some(tracker))
        && (entry.after..entry.before).contains(&observation.local_time())
}

fn summary_value<'a>(summary: &'a Table, path: &str) -> Option<&'a Value> {
    let mut segments = path.split('.');
    let mut value = summary.get(segments.next()?)?;
    for segment in segments {
        value = match value {
            Value::Table(table) => table.get(segment)?,
            Value::Array(array) if segment.bytes().all(|byte| byte.is_ascii_digit()) => {
                array.get(segment.parse::<usize>().ok()?)?
            }
            _ => return None,
        };
    }
    Some(value)
}

fn describe_expected(entry: &ExpectedDetection) -> String {
    format!(
        "{} トラッカー {} [{} から {} の直前まで]",
        entry.rule,
        tracker_label(entry.tracker, "任意"),
        LogTime(entry.after.0),
        LogTime(entry.before.0),
    )
}

fn describe_observation(entry: &RaisedDetection<'_>) -> String {
    format!(
        "{} {} トラッカー {}",
        LogTime(entry.local_time().0),
        entry.rule,
        tracker_label(entry.tracker, "不明"),
    )
}

fn tracker_label(tracker: Option<u16>, missing: &str) -> String {
    tracker.map_or_else(|| missing.to_owned(), |number| number.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_DETECTION: &str = r#"
check_rules = ["R1"]

[[detection]]
rule = "R1"
tracker = 2
after = "2026-11-28 12:00:00"
before = "2026-11-28 12:00:01"
"#;

    fn raised(rule: &str, tracker: u16, ms: i64) -> RaisedDetection<'_> {
        let offset = UtcOffset(9 * 60);
        let at = LogTime::from_local(
            LogDate {
                year: 2026,
                month: 11,
                day: 28,
            },
            12,
            0,
            0,
            0,
            offset,
        )
        .expect("valid test time");
        RaisedDetection {
            rule,
            tracker: Some(tracker),
            at: LogTime(at.0 + ms),
            offset,
        }
    }

    #[test]
    fn matches_expected_local_time_and_tracker() {
        let expected = Expectations::parse(ONE_DETECTION).unwrap();
        assert_eq!(expected.replay.flush_s, 600);
        assert!(!expected.replay.catch_up);
        assert!(
            expected
                .compare(&[raised("R1", 2, 500)], &BTreeMap::new())
                .is_ok()
        );
    }

    #[test]
    fn reports_missing_detection() {
        let expected = Expectations::parse(ONE_DETECTION).unwrap();
        let report = expected.compare(&[], &BTreeMap::new()).unwrap_err();
        assert!(report.contains("見つからない検出　R1 トラッカー 2"));
    }

    #[test]
    fn reports_surplus_detections_with_expected_and_actual_lists() {
        let expected = Expectations::parse(ONE_DETECTION).unwrap();
        let report = expected
            .compare(
                &[raised("R1", 2, 900), raised("R1", 2, 500)],
                &BTreeMap::new(),
            )
            .unwrap_err();
        assert!(report.contains("余分な検出　2026-11-28 12:00:00.900 R1 トラッカー 2"));
        assert!(report.contains("期待する検出"));
        let observed = report.split_once("実際の検出").unwrap().1;
        assert!(observed.find("00.500").unwrap() < observed.find("00.900").unwrap());
    }

    #[test]
    fn rejects_wrong_tracker_number() {
        let expected = Expectations::parse(ONE_DETECTION).unwrap();
        let report = expected
            .compare(&[raised("R1", 3, 500)], &BTreeMap::new())
            .unwrap_err();
        assert!(report.contains("見つからない検出　R1 トラッカー 2"));
        assert!(report.contains("余分な検出　2026-11-28 12:00:00.500 R1 トラッカー 3"));
    }

    #[test]
    fn reserves_same_time_detections_for_explicit_trackers_in_either_order() {
        for tracker in [1, 2] {
            let expected = Expectations::parse(&format!(
                r#"
check_rules = ["R1"]

[[detection]]
rule = "R1"
after = "2026-11-28 12:00:00"
before = "2026-11-28 12:00:02"

[[detection]]
rule = "R1"
tracker = {tracker}
after = "2026-11-28 12:00:00.250"
before = "2026-11-28 12:00:01"
"#
            ))
            .unwrap();
            for [first, second] in [[1, 2], [2, 1]] {
                let observed = [raised("R1", first, 500), raised("R1", second, 500)];
                assert_eq!(expected.compare(&observed, &BTreeMap::new()), Ok(()));
            }
        }
    }

    #[test]
    fn reassigns_an_earlier_detection_when_only_it_fits_the_shorter_window() {
        let expected = Expectations::parse(
            r#"
check_rules = ["R1"]

[[detection]]
rule = "R1"
tracker = 2
after = "2026-11-28 12:00:00"
before = "2026-11-28 12:00:02"

[[detection]]
rule = "R1"
after = "2026-11-28 12:00:00.250"
before = "2026-11-28 12:00:00.600"
"#,
        )
        .unwrap();
        let observed = [raised("R1", 2, 500), raised("R1", 2, 900)];
        assert_eq!(expected.compare(&observed, &BTreeMap::new()), Ok(()));
    }

    #[test]
    fn includes_after_boundary_and_excludes_before_boundary() {
        let expected = Expectations::parse(ONE_DETECTION).unwrap();
        assert!(
            expected
                .compare(&[raised("R1", 2, 0)], &BTreeMap::new())
                .is_ok()
        );
        assert!(
            expected
                .compare(&[raised("R1", 2, 1_000)], &BTreeMap::new())
                .is_err()
        );
    }

    #[test]
    fn disregards_rules_outside_check_rules() {
        let expected = Expectations::parse(ONE_DETECTION).unwrap();
        assert!(
            expected
                .compare(
                    &[raised("R2", 2, 500), raised("R1", 2, 500)],
                    &BTreeMap::new()
                )
                .is_ok()
        );
    }

    #[test]
    fn compares_only_named_summary_values() {
        let expected = Expectations::parse(
            r#"
check_rules = ["R1"]
[summary."2026-11-28"]
"faults.wifi_info_missing" = 1
"#,
        )
        .unwrap();
        let mut observed: ObservedSummaries = toml::from_str(
            r#"
["2026-11-28".faults]
wifi_info_missing = 1
other = 12
"#,
        )
        .unwrap();
        assert_eq!(expected.compare(&[], &observed), Ok(()));
        observed
            .get_mut("2026-11-28")
            .unwrap()
            .get_mut("faults")
            .unwrap()
            .as_table_mut()
            .unwrap()
            .insert("wifi_info_missing".into(), Value::Integer(2));
        assert!(
            expected
                .compare(&[], &observed)
                .unwrap_err()
                .contains("期待 1　実際 2")
        );
    }

    #[test]
    fn compares_one_tracker_summary_field_after_midnight() {
        let expected = Expectations::parse(
            r#"
check_rules = []
[summary."2026-11-28"]
"trackers.0.tracking_s" = 1530.0
"#,
        )
        .unwrap();
        // §8.1's next-day-only replay, with an extra tracker and a prior day
        // that this expectation does not check.
        let summaries = |tracking_s| {
            toml::from_str::<ObservedSummaries>(&format!(
                r#"
["2026-11-27"]
trackers = [{{ key = 1, tracking_s = 240.0 }}]

["2026-11-28"]
unobserved_s = 240.0
trackers = [
    {{ key = 1, connected_s = 1560.0, tracking_s = {tracking_s} }},
    {{ key = 2, tracking_s = 1530.0 }},
]
"#
            ))
            .unwrap()
        };
        assert_eq!(expected.compare(&[], &summaries("1530.0")), Ok(()));
        let report = expected.compare(&[], &summaries("240.0")).unwrap_err();
        assert!(report.contains("集計 2026-11-28 trackers.0.tracking_s　期待 1530.0　実際 240.0"));
    }
}
