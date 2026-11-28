use std::io::{self, Write};

use openut_core::notice::Notice;
use openut_core::rules::{R5Config, RuleId, Step};
use openut_core::tracker::{HubNumber, TrackerLabel};

use super::NoticeSink;

pub struct Console<W> {
    out: W,
    tracking_wait_s: u64,
}

impl<W: Write> Console<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            tracking_wait_s: R5Config::default().wait_s,
        }
    }

    pub fn with_tracking_wait_s(mut self, seconds: u64) -> Self {
        self.tracking_wait_s = seconds;
        self
    }
}

impl<W: Write> NoticeSink for Console<W> {
    fn push(&mut self, notice: &Notice) -> io::Result<()> {
        if !notice.push {
            return Ok(());
        }
        let target = tracker_label(&notice.target);
        let (mut headline, action) = match (notice.rule, notice.step) {
            (RuleId::R1, step @ (Step::Raised | Step::Escalated)) => (
                if step == Step::Raised {
                    "トラッカーの追跡が始まりません".into()
                } else {
                    "まだ追跡が始まりません".into()
                },
                Some(format!(
                    "{target}の電源だけを入れ直してください。ほかのトラッカーとドングルはそのままにしてください。"
                )),
            ),
            (RuleId::R1, Step::Resolved) => ("追跡が始まりました".into(), None),
            (RuleId::R2, Step::Raised) => {
                let host = notice
                    .host
                    .as_ref()
                    .map_or_else(|| "ホストのトラッカー".into(), tracker_label);
                (
                    format!("{target}の準備に時間がかかっています"),
                    Some(format!("{target}を{host}の近くに置いて待ってください。")),
                )
            }
            (RuleId::R3, Step::Raised) => (
                format!("{target}の追跡が始まりません"),
                Some(format!(
                    "{target}を手に持ち、部屋の中をゆっくり見せてください。"
                )),
            ),
            (RuleId::R4, Step::Raised) => (
                format!("{target}の接続が切れました"),
                Some(format!("{target}の電源が入っているか確認してください。")),
            ),
            (RuleId::R4, Step::Resolved) => (format!("{target}がつながりました"), None),
            (RuleId::R5, Step::Raised) => (
                format!(
                    "{target}の追跡が{}以上止まっています",
                    duration(self.tracking_wait_s)
                ),
                Some(format!(
                    "{target}のカメラの前をあけ、明るい方へ向けてください。"
                )),
            ),
            _ => return Ok(()),
        };
        if notice.repetitions > 1 {
            headline.push_str(&format!("（{} 回）", notice.repetitions));
        }
        if notice.other_count > 0 {
            headline.push_str(&format!("（ほか {} 件）", notice.other_count));
        }
        if let Some(action) = action {
            writeln!(self.out, "{headline}。{action}")?;
        } else {
            writeln!(self.out, "{headline}。")?;
        }
        self.out.flush()
    }
}

fn tracker_label(label: &TrackerLabel) -> String {
    match (&label.serial_tail, label.hub_number) {
        (Some(tail), Some(HubNumber(number))) => format!("トラッカー {number}（…{tail}）"),
        (Some(tail), None) => format!("トラッカー（…{tail}）"),
        (None, _) if label.host => "ホストのトラッカー".into(),
        (None, _) => "トラッカー".into(),
    }
}

fn duration(seconds: u64) -> String {
    if seconds >= 60 && seconds.is_multiple_of(60) {
        format!("{} 分", seconds / 60)
    } else {
        format!("{seconds} 秒")
    }
}

#[cfg(test)]
mod tests {
    use openut_core::{time::LogTime, tracker::Serial};

    use super::*;

    fn notice(rule: RuleId) -> Notice {
        Notice {
            rule,
            tracker: None,
            episode: 1,
            serial: Some(Serial("SYNTHETIC-0001".into())),
            conn: None,
            step: Step::Raised,
            at: LogTime(0),
            target: TrackerLabel {
                hub_number: Some(HubNumber(2)),
                serial_tail: Some("0001".into()),
                host: false,
            },
            host: None,
            repetitions: 1,
            other_count: 0,
            push: true,
            priority: 1,
        }
    }

    #[test]
    fn writes_one_japanese_line_with_the_grouped_frozen_label() {
        let mut output = Vec::new();
        let mut notice = notice(RuleId::R4);
        notice.repetitions = 2;
        notice.other_count = 3;
        let mut console = Console::new(&mut output);
        console.push(&notice).unwrap();
        notice.push = false;
        console.push(&notice).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "トラッカー 2（…0001）の接続が切れました（2 回）（ほか 3 件）。トラッカー 2（…0001）の電源が入っているか確認してください。\n"
        );
    }

    #[test]
    fn describes_the_configured_tracking_wait() {
        let mut output = Vec::new();
        Console::new(&mut output)
            .with_tracking_wait_s(45)
            .push(&notice(RuleId::R5))
            .unwrap();
        assert!(String::from_utf8(output).unwrap().contains("45 秒以上"));
    }
}
