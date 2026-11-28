//! R1: consecutive unanswered Wi-Fi requests and host recovery.

use crate::event::{Event, Timed};
use crate::state::{HostWifi, Phase, State, TrackerView};
use crate::time::LogTime;

use super::{Detection, R1Config, RuleId, Step, Target, after, new_episode};

#[derive(Default)]
pub(super) struct R1 {
    series: Option<Series>,
    episode: Option<Episode>,
}

struct Series {
    first: LogTime,
    last: LogTime,
    count: u64,
}

struct Episode {
    id: u64,
    target: Target,
    quiet_at: LogTime,
    escalate_at: Option<LogTime>,
    renewed: bool,
    queried: bool,
}

impl Episode {
    fn detection(&self, at: LogTime, step: Step) -> Detection {
        let mut detection = self.target.clone().detection(RuleId::R1, self.id, at, step);
        detection.host_wifi_queried = Some(self.queried);
        detection
    }
}

impl R1 {
    pub(super) fn apply(
        &mut self,
        ev: &Timed,
        state: &State,
        cfg: &R1Config,
        output: &mut Vec<Detection>,
        next_episode: &mut u64,
    ) {
        match ev.event {
            Event::HostWifiObtained { .. } | Event::WifiAssigned { .. } => {
                self.finish(ev.at, Step::Resolved, output);
            }
            Event::ServerStarted | Event::ServerStopped | Event::DongleRemoved => {
                self.finish(ev.at, Step::Ended, output);
            }
            Event::TrackerRemoved { .. } if !state.trackers().any(connected) => {
                self.finish(ev.at, Step::Ended, output);
            }
            Event::HostAssigned { .. } => self.series = None,
            Event::WifiRequestUnanswered { .. } => {
                self.request(ev.at, state, cfg, output, next_episode);
            }
            _ => {}
        }
    }

    pub(super) fn deadline(&self) -> Option<LogTime> {
        let episode = self.episode.as_ref()?;
        Some(match episode.escalate_at {
            Some(at) => at.min(episode.quiet_at),
            None => episode.quiet_at,
        })
    }

    pub(super) fn fire(&mut self, at: LogTime, state: &State, output: &mut Vec<Detection>) {
        let Some(episode) = self.episode.as_mut() else {
            return;
        };
        // At equal times, the original quiet timer precedes escalation. A quiet
        // timer replaced by a subsequent request comes after escalation.
        if episode.escalate_at == Some(at)
            && (at < episode.quiet_at || (at == episode.quiet_at && episode.renewed))
        {
            episode.escalate_at = None;
            episode.target = host_target(state);
            episode.queried = host_wifi_queried(state);
            output.push(episode.detection(at, Step::Escalated));
        } else if at == episode.quiet_at {
            self.finish(at, Step::Ended, output);
        }
    }

    fn request(
        &mut self,
        at: LogTime,
        state: &State,
        cfg: &R1Config,
        output: &mut Vec<Detection>,
        next_episode: &mut u64,
    ) {
        if matches!(state.host_wifi(), HostWifi::Held { .. }) {
            return;
        }
        if let Some(episode) = &mut self.episode {
            episode.quiet_at = after(at, cfg.quiet_s);
            episode.renewed = true;
            return;
        }

        let series = self.series.get_or_insert(Series {
            first: at,
            last: at,
            count: 0,
        });
        if at > after(series.last, cfg.gap_s) {
            series.first = at;
            series.count = 0;
        }
        series.last = at;
        series.count = series.count.saturating_add(1);
        if series.count < cfg.count || at < after(series.first, cfg.span_s) {
            return;
        }

        let episode = Episode {
            id: new_episode(next_episode),
            target: host_target(state),
            quiet_at: after(at, cfg.quiet_s),
            escalate_at: Some(after(at, cfg.escalate_s)),
            renewed: false,
            queried: host_wifi_queried(state),
        };
        output.push(episode.detection(at, Step::Raised));
        self.series = None;
        self.episode = Some(episode);
    }

    fn finish(&mut self, at: LogTime, step: Step, output: &mut Vec<Detection>) {
        self.series = None;
        if let Some(episode) = self.episode.take() {
            output.push(episode.detection(at, step));
        }
    }
}

fn host_wifi_queried(state: &State) -> bool {
    matches!(state.host_wifi(), HostWifi::Missing { queried: true, .. })
}

fn connected(tracker: &TrackerView) -> bool {
    tracker.phase != Phase::Disconnected
}

fn host_target(state: &State) -> Target {
    let Some(host) = state.host() else {
        return Target::default();
    };
    if let Some(view) = state.tracker(host.key)
        && connected(view)
        && view.conn == host.conn
    {
        return target_for(view);
    }
    let Some(serial) = host.serial.as_ref() else {
        return Target::default();
    };
    if let Some(view) = state
        .trackers()
        .find(|view| connected(view) && view.serial.as_ref() == Some(serial))
    {
        return target_for(view);
    }
    Target {
        serial: Some(serial.clone()),
        ..Target::default()
    }
}

fn target_for(view: &TrackerView) -> Target {
    Target {
        tracker: Some(view.key),
        serial: view.serial.clone(),
        conn: Some(view.conn),
    }
}

#[cfg(test)]
mod tests {
    use crate::time::UtcOffset;
    use crate::tracker::{Serial, TrackerKey};

    use super::*;

    const HOST: TrackerKey = TrackerKey(2);
    const OTHER: TrackerKey = TrackerKey(4);

    #[derive(Default)]
    struct Fixture {
        rule: R1,
        state: State,
        cfg: R1Config,
        output: Vec<Detection>,
        episode: u64,
    }

    impl Fixture {
        fn apply(&mut self, at_ms: i64, event: Event) {
            self.advance(at_ms);
            let ev = Timed {
                at: LogTime(at_ms),
                offset: UtcOffset(0),
                event,
            };
            self.state.apply(&ev);
            self.rule.apply(
                &ev,
                &self.state,
                &self.cfg,
                &mut self.output,
                &mut self.episode,
            );
        }

        fn advance(&mut self, at_ms: i64) {
            while let Some(deadline) = self.rule.deadline()
                && deadline < LogTime(at_ms)
            {
                self.rule.fire(deadline, &self.state, &mut self.output);
            }
        }

        fn request(&mut self, at_ms: i64) {
            self.apply(at_ms, Event::WifiRequestUnanswered { tracker: None });
        }

        fn requests(&mut self, start_ms: i64, len_ms: i64) {
            for offset in (0..=len_ms).step_by(550) {
                self.request(start_ms + offset);
            }
        }

        fn assign_host(&mut self, at_ms: i64, tracker: TrackerKey) {
            self.apply(at_ms, Event::HostAssigned { tracker });
        }

        fn add(&mut self, at_ms: i64, tracker: TrackerKey) {
            self.apply(
                at_ms,
                Event::TrackerAdded {
                    tracker,
                    usb: false,
                },
            );
        }

        fn remove(&mut self, at_ms: i64, tracker: TrackerKey) {
            self.apply(
                at_ms,
                Event::TrackerRemoved {
                    tracker,
                    usb: false,
                },
            );
        }

        fn serial(&mut self, at_ms: i64, tracker: TrackerKey) {
            self.apply(
                at_ms,
                Event::SerialKnown {
                    tracker,
                    serial: synthetic_serial(),
                    hub_number: None,
                    firmware: None,
                },
            );
        }

        fn stages(&self) -> Vec<(Step, LogTime)> {
            self.output
                .iter()
                .map(|detection| (detection.step, detection.at))
                .collect()
        }
    }

    #[test]
    fn reports_host_query_evidence_without_using_it_as_a_condition() {
        for queried in [false, true] {
            let mut case = Fixture::default();
            case.add(0, HOST);
            case.assign_host(1, HOST);
            if queried {
                case.apply(2, Event::HostWifiQueried { host: HOST });
            }
            case.requests(10, 16_000);
            assert_eq!(case.stages(), [(Step::Raised, LogTime(15_410))]);
            assert_eq!(case.output[0].host_wifi_queried, Some(queried));
        }
    }

    fn synthetic_serial() -> Serial {
        Serial("SYNTHETIC0001".into())
    }

    #[test]
    fn series_needs_count_and_span_and_includes_the_exact_gap() {
        let mut fixture = Fixture::default();
        fixture.request(0);
        fixture.request(10_000);
        fixture.request(14_999);
        assert!(fixture.output.is_empty());
        fixture.request(15_000);
        assert_eq!(fixture.stages(), [(Step::Raised, LogTime(15_000))]);

        let mut fixture = Fixture::default();
        fixture.cfg.span_s = 1;
        fixture.request(0);
        fixture.request(1_000);
        assert!(fixture.output.is_empty());
        fixture.request(1_001);
        assert_eq!(fixture.stages(), [(Step::Raised, LogTime(1_001))]);
    }

    #[test]
    fn requests_separated_by_more_than_the_gap_start_a_new_series() {
        for gap in [10_001, 61_000] {
            let mut fixture = Fixture::default();
            fixture.request(0);
            fixture.request(500);
            fixture.request(500 + gap);
            fixture.request(1_000 + gap);
            assert!(fixture.output.is_empty());
            fixture.requests(1_500 + gap, 15_400);
            assert_eq!(fixture.output.len(), 1);
        }
    }

    #[test]
    fn report_host_starts_without_wifi_evidence_and_repeated_requests_raise_once() {
        let mut fixture = Fixture::default();
        fixture.apply(
            0,
            Event::Status {
                tracker: HOST,
                connected: true,
                host: true,
                map_ready: false,
                usb: None,
                battery: None,
            },
        );
        for tracker in [TrackerKey(0), OTHER] {
            fixture.apply(
                0,
                Event::Status {
                    tracker,
                    connected: true,
                    host: false,
                    map_ready: false,
                    usb: None,
                    battery: None,
                },
            );
        }
        assert_eq!(fixture.state.host_wifi(), HostWifi::Unknown);
        fixture.requests(550, 60_000);
        assert_eq!(fixture.stages(), [(Step::Raised, LogTime(15_950))]);
        assert_eq!(fixture.output[0].tracker, Some(HOST));
        assert_eq!(
            fixture.output[0].conn,
            fixture.state.tracker(HOST).map(|tracker| tracker.conn)
        );
    }

    #[test]
    fn host_reassignment_keeps_the_episode_but_wifi_recovery_ends_it() {
        let mut fixture = Fixture::default();
        fixture.add(0, HOST);
        fixture.assign_host(0, HOST);
        fixture.requests(550, 20_000);
        assert_eq!(fixture.stages(), [(Step::Raised, LogTime(15_950))]);

        fixture.add(21_000, OTHER);
        fixture.assign_host(21_000, OTHER);
        fixture.requests(21_550, 20_000);
        assert_eq!(fixture.output.len(), 1);
        fixture.apply(42_000, Event::HostWifiObtained { host: None });
        fixture.requests(42_550, 60_000);
        assert_eq!(
            fixture.stages(),
            [
                (Step::Raised, LogTime(15_950)),
                (Step::Resolved, LogTime(42_000))
            ],
        );
        assert_eq!(fixture.output[0].episode, fixture.output[1].episode);
        assert_eq!(fixture.output[1].tracker, Some(HOST));
        assert_eq!(fixture.output[0].conn, fixture.output[1].conn);
    }

    #[test]
    fn wifi_silence_ends_at_the_deadline_and_starts_a_fresh_series() {
        let mut fixture = Fixture::default();
        fixture.cfg.count = 2;
        fixture.cfg.span_s = 2;
        fixture.cfg.quiet_s = 4;
        fixture.request(0);
        fixture.request(2_000);
        fixture.request(6_000);
        assert_eq!(fixture.output.len(), 1);
        fixture.advance(10_001);
        assert_eq!(
            fixture.stages(),
            [
                (Step::Raised, LogTime(2_000)),
                (Step::Ended, LogTime(10_000))
            ],
        );
        fixture.request(10_001);
        assert_eq!(fixture.output.len(), 2);
        fixture.request(12_001);
        assert_eq!(fixture.output[2].step, Step::Raised);
        assert_ne!(fixture.output[0].episode, fixture.output[2].episode);
    }

    #[test]
    fn resolving_at_escalation_deadline_suppresses_escalation() {
        for delay in [0, 1] {
            let mut fixture = Fixture::default();
            fixture.cfg.count = 1;
            fixture.cfg.span_s = 0;
            fixture.cfg.escalate_s = 3;
            fixture.request(0);
            fixture.apply(3_000 + delay, Event::HostWifiObtained { host: None });
            let expected = if delay == 0 {
                vec![(Step::Raised, LogTime(0)), (Step::Resolved, LogTime(3_000))]
            } else {
                vec![
                    (Step::Raised, LogTime(0)),
                    (Step::Escalated, LogTime(3_000)),
                    (Step::Resolved, LogTime(3_001)),
                ]
            };
            assert_eq!(fixture.stages(), expected);
        }
    }

    #[test]
    fn deadlines_of_the_same_episode_tie_in_timer_creation_order() {
        let mut initial_tie = Fixture::default();
        initial_tie.cfg.count = 1;
        initial_tie.cfg.span_s = 0;
        initial_tie.cfg.quiet_s = 5;
        initial_tie.cfg.escalate_s = 5;
        initial_tie.request(0);
        initial_tie.advance(5_001);
        assert_eq!(
            initial_tie.stages(),
            [(Step::Raised, LogTime(0)), (Step::Ended, LogTime(5_000))]
        );

        let mut renewed_tie = Fixture::default();
        renewed_tie.cfg.count = 1;
        renewed_tie.cfg.span_s = 0;
        renewed_tie.cfg.quiet_s = 2;
        renewed_tie.cfg.escalate_s = 3;
        renewed_tie.request(0);
        renewed_tie.request(1_000);
        renewed_tie.advance(3_001);
        assert_eq!(
            renewed_tie.stages(),
            [
                (Step::Raised, LogTime(0)),
                (Step::Escalated, LogTime(3_000)),
                (Step::Ended, LogTime(3_000))
            ]
        );
    }

    #[test]
    fn host_assignment_recovery_and_topology_changes_reset_the_request_series() {
        for reset in [
            Event::HostAssigned { tracker: HOST },
            Event::HostWifiObtained { host: None },
            Event::WifiAssigned { tracker: OTHER },
            Event::ServerStarted,
            Event::ServerStopped,
            Event::DongleRemoved,
            Event::TrackerRemoved {
                tracker: HOST,
                usb: false,
            },
        ] {
            let mut fixture = Fixture::default();
            fixture.add(0, HOST);
            fixture.request(1);
            fixture.request(8_000);
            fixture.apply(10_000, reset);
            fixture.request(16_000);
            assert!(fixture.output.is_empty());
        }
    }

    #[test]
    fn dongle_removal_ends_r1_even_when_usb_remains_connected() {
        let mut fixture = Fixture::default();
        fixture.apply(
            0,
            Event::TrackerAdded {
                tracker: OTHER,
                usb: true,
            },
        );
        fixture.requests(550, 20_000);
        fixture.apply(21_000, Event::DongleRemoved);
        assert_eq!(
            fixture.stages(),
            [
                (Step::Raised, LogTime(15_950)),
                (Step::Ended, LogTime(21_000))
            ]
        );
        assert!(fixture.state.tracker(OTHER).is_some_and(connected));
    }

    #[test]
    fn a_reconnect_does_not_reset_the_r1_series() {
        let mut fixture = Fixture::default();
        fixture.add(0, HOST);
        fixture.assign_host(0, HOST);
        fixture.request(0);
        fixture.request(8_000);
        fixture.add(10_000, HOST);
        fixture.request(15_000);
        assert_eq!(fixture.stages(), [(Step::Raised, LogTime(15_000))]);
        assert_eq!(fixture.output[0].tracker, None);
        assert_eq!(fixture.output[0].conn, None);
    }

    #[test]
    fn slot_reuse_never_targets_the_replacement_but_reconnect_by_serial_can() {
        for (known, reconnected) in [(false, false), (true, false), (true, true)] {
            let mut fixture = Fixture::default();
            fixture.add(0, HOST);
            fixture.assign_host(0, HOST);
            if known {
                fixture.serial(1, HOST);
            }
            fixture.remove(2, HOST);
            fixture.add(3, HOST);
            if reconnected {
                fixture.add(4, OTHER);
                fixture.serial(5, OTHER);
            }
            fixture.requests(550, 20_000);
            assert_eq!(fixture.output.len(), 1);
            let expected = if reconnected {
                Target {
                    tracker: Some(OTHER),
                    serial: Some(synthetic_serial()),
                    conn: fixture.state.tracker(OTHER).map(|tracker| tracker.conn),
                }
            } else {
                Target {
                    serial: known.then(synthetic_serial),
                    ..Target::default()
                }
            };
            let raised = &fixture.output[0];
            assert_eq!(raised.tracker, expected.tracker);
            assert_eq!(raised.serial, expected.serial);
            assert_eq!(raised.conn, expected.conn);
        }
    }

    #[test]
    fn escalation_resolves_host_again_and_freezes_its_latest_target() {
        for reconnected in [false, true] {
            let mut fixture = Fixture::default();
            fixture.cfg.escalate_s = 25;
            fixture.add(0, HOST);
            fixture.assign_host(0, HOST);
            fixture.add(0, OTHER);
            fixture.requests(550, 20_000);
            let raised = &fixture.output[0];
            assert_eq!(raised.tracker, Some(HOST));
            assert_eq!(raised.serial, None);
            assert_eq!(
                raised.conn,
                fixture.state.tracker(HOST).map(|tracker| tracker.conn)
            );

            fixture.serial(21_000, HOST);
            fixture.remove(22_000, HOST);
            fixture.add(23_000, HOST);
            if reconnected {
                fixture.serial(24_000, OTHER);
            }
            fixture.advance(40_951);
            let bumped = &fixture.output[1];
            assert_eq!(bumped.step, Step::Escalated);
            assert_eq!(bumped.at, LogTime(40_950));
            assert_eq!(bumped.episode, fixture.output[0].episode);
            assert_eq!(bumped.serial, Some(synthetic_serial()));
            assert_eq!(bumped.tracker, reconnected.then_some(OTHER));
            let expected_conn = if reconnected {
                fixture.state.tracker(OTHER).map(|tracker| tracker.conn)
            } else {
                None
            };
            assert_eq!(bumped.conn, expected_conn);

            fixture.serial(41_000, HOST);
            fixture.apply(42_000, Event::HostWifiObtained { host: None });
            let resolved = &fixture.output[2];
            assert_eq!(resolved.step, Step::Resolved);
            assert_eq!(resolved.tracker, reconnected.then_some(OTHER));
            assert_eq!(resolved.serial, Some(synthetic_serial()));
            assert_eq!(resolved.conn, expected_conn);
        }
    }
}
