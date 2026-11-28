//! Map synchronization, initial alignment, and tracking loss (design §6.2).

use std::collections::BTreeMap;

use crate::event::{Event, Timed};
use crate::state::{ConnId, Phase, Role, State, TrackerView};
use crate::time::LogTime;
use crate::tracker::TrackerKey;

use super::{Detection, RuleId, RulesConfig, Step, Target, after, new_episode};

#[derive(Default)]
pub(super) struct Tracking {
    watches: BTreeMap<(RuleId, TrackerKey), Watch>,
}

struct Watch {
    conn: ConnId,
    status: Status,
}

enum Status {
    Waiting(LogTime),
    Raised { episode: u64, target: Target },
}

impl Tracking {
    pub(super) fn apply(
        &mut self,
        ev: &Timed,
        state: &State,
        cfg: &RulesConfig,
        output: &mut Vec<Detection>,
    ) {
        self.watches.retain(|&(rule, key), watch| {
            let step = state
                .tracker(key)
                .filter(|view| view.conn == watch.conn && view.phase != Phase::Disconnected)
                .map_or(Some(Step::Ended), |view| outcome(rule, view, cfg));
            let Some(step) = step else {
                return true;
            };
            if let Status::Raised { episode, target } = &watch.status {
                output.push(target.clone().detection(rule, *episode, ev.at, step));
            }
            false
        });

        match ev.event {
            Event::ClientLink {
                tracker,
                connected: true,
            } => {
                if let Some(view) = state.tracker(tracker)
                    && view.role == Role::Client
                    && view.phase == Phase::MapSyncing
                    && view.since == ev.at
                {
                    self.wait(RuleId::R2, view, after(ev.at, cfg.r2.wait_s));
                }
            }
            Event::HostAssigned { tracker } => {
                if let Some(view) = state.tracker(tracker)
                    && view.role == Role::Host
                    && !view.inferred
                    && !view.tracked_since_connect
                {
                    self.wait(RuleId::R3, view, after(ev.at, cfg.r3.wait_s));
                }
            }
            Event::Status { tracker, .. }
            | Event::Pose { tracker, .. }
            | Event::Oet { tracker, .. } => {
                let Some(view) = state
                    .tracker(tracker)
                    .filter(|view| view.phase == Phase::Relocalizing && view.since == ev.at)
                else {
                    return;
                };
                // A connection that already tracked can only leave Tracking for Relocalizing.
                if view.tracked_since_connect {
                    if cfg.r5.enabled {
                        self.wait(RuleId::R5, view, after(ev.at, cfg.r5.wait_s));
                    }
                } else if cfg.r3.clients && view.role == Role::Client && !view.inferred {
                    self.wait(RuleId::R3, view, after(ev.at, cfg.r3.wait_s));
                }
            }
            _ => {}
        }
    }

    fn wait(&mut self, rule: RuleId, tracker: &TrackerView, deadline: LogTime) {
        self.watches.entry((rule, tracker.key)).or_insert(Watch {
            conn: tracker.conn,
            status: Status::Waiting(deadline),
        });
    }

    pub(super) fn deadline(&self) -> Option<(LogTime, RuleId, TrackerKey)> {
        self.watches
            .iter()
            .filter_map(|(&(rule, key), watch)| match watch.status {
                Status::Waiting(at) => Some((at, rule, key)),
                Status::Raised { .. } => None,
            })
            .min()
    }

    pub(super) fn fire(
        &mut self,
        at: LogTime,
        state: &State,
        output: &mut Vec<Detection>,
        next_episode: &mut u64,
    ) {
        let Some((_, rule, key)) = self.deadline().filter(|(deadline, _, _)| *deadline == at)
        else {
            return;
        };
        let Some(view) = state.tracker(key) else {
            self.watches.remove(&(rule, key));
            return;
        };
        let episode = new_episode(next_episode);
        let target = Target {
            tracker: Some(key),
            serial: view.serial.clone(),
            conn: Some(view.conn),
        };
        output.push(target.clone().detection(rule, episode, at, Step::Raised));
        self.watches.insert(
            (rule, key),
            Watch {
                conn: view.conn,
                status: Status::Raised { episode, target },
            },
        );
    }
}

fn outcome(rule: RuleId, view: &TrackerView, cfg: &RulesConfig) -> Option<Step> {
    match (rule, view.phase) {
        (RuleId::R2, _) if view.role != Role::Client => Some(Step::Ended),
        (RuleId::R2, Phase::Relocalizing | Phase::Tracking)
        | (RuleId::R3 | RuleId::R5, Phase::Tracking) => Some(Step::Resolved),
        (RuleId::R2, Phase::WaitingWifi) => Some(Step::Ended),
        (RuleId::R3, _) if !cfg.r3.clients && view.role != Role::Host => Some(Step::Ended),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::{Engine, EngineConfig, Output};
    use crate::event::PoseStatus;
    use crate::time::UtcOffset;
    use crate::tracker::Serial;

    use super::*;

    fn apply(engine: &mut Engine, at: i64, event: Event) {
        engine.apply(Timed {
            at: LogTime(at),
            offset: UtcOffset(0),
            event,
        });
    }

    fn added(tracker: u16) -> Event {
        Event::TrackerAdded {
            tracker: TrackerKey(tracker),
            usb: false,
        }
    }

    fn host_assigned(tracker: u16) -> Event {
        Event::HostAssigned {
            tracker: TrackerKey(tracker),
        }
    }

    fn link(tracker: u16, connected: bool) -> Event {
        Event::ClientLink {
            tracker: TrackerKey(tracker),
            connected,
        }
    }

    fn map_ready(tracker: u16, host: bool) -> Event {
        Event::Status {
            tracker: TrackerKey(tracker),
            connected: true,
            host,
            map_ready: true,
            battery: None,
            usb: None,
        }
    }

    fn pose(tracker: u16, status: PoseStatus) -> Event {
        Event::Pose {
            tracker: TrackerKey(tracker),
            status,
        }
    }

    fn serial_known(tracker: u16) -> Event {
        Event::SerialKnown {
            tracker: TrackerKey(tracker),
            serial: Serial("SYNTHETIC-01".into()),
            hub_number: None,
            firmware: None,
        }
    }

    fn detections(engine: &mut Engine) -> Vec<Detection> {
        engine
            .drain()
            .into_iter()
            .filter_map(|output| match output {
                Output::Detection(detection) => Some(detection),
                Output::Notice(_) | Output::DayClosed(_) => None,
            })
            .collect()
    }

    fn steps(detections: &[Detection]) -> Vec<(RuleId, u16, Step, i64)> {
        detections
            .iter()
            .map(|detection| {
                (
                    detection.rule,
                    detection.tracker.expect("a tracker rule has a target").0,
                    detection.step,
                    detection.at.0,
                )
            })
            .collect()
    }

    #[test]
    fn r2_suppresses_a_map_received_at_the_deadline() {
        for delay in [0, 1] {
            let mut engine = Engine::new(EngineConfig::default());
            apply(&mut engine, 0, added(1));
            apply(&mut engine, 100, link(1, true));
            engine.advance_to(LogTime(30_100));
            assert!(detections(&mut engine).is_empty());

            apply(&mut engine, 30_100 + delay, map_ready(1, false));
            engine.advance_to(LogTime(31_000));
            let output = detections(&mut engine);
            if delay == 0 {
                assert!(output.is_empty());
            } else {
                assert_eq!(
                    steps(&output),
                    [
                        (RuleId::R2, 1, Step::Raised, 30_100),
                        (RuleId::R2, 1, Step::Resolved, 30_101),
                    ]
                );
            }
        }
    }

    #[test]
    fn becoming_host_at_the_r2_deadline_cancels_the_wait() {
        let mut engine = Engine::new(EngineConfig::default());
        apply(&mut engine, 0, added(2));
        apply(&mut engine, 1_000, link(2, true));
        engine.advance_to(LogTime(31_000));
        apply(&mut engine, 31_000, host_assigned(2));
        engine.advance_to(LogTime(32_000));

        assert!(detections(&mut engine).is_empty());
    }

    #[test]
    fn becoming_host_ends_an_already_raised_r2_episode() {
        for switch in [host_assigned(2), map_ready(2, true)] {
            let mut engine = Engine::new(EngineConfig::default());
            apply(&mut engine, 0, added(2));
            apply(&mut engine, 1_000, link(2, true));
            engine.advance_to(LogTime(31_001));
            let raised = detections(&mut engine);
            assert_eq!(steps(&raised), [(RuleId::R2, 2, Step::Raised, 31_000)]);

            apply(&mut engine, 32_000, switch);

            let ended = detections(&mut engine);
            assert_eq!(steps(&ended), [(RuleId::R2, 2, Step::Ended, 32_000)]);
            assert_eq!(ended[0].episode, raised[0].episode);
            assert_eq!(ended[0].conn, raised[0].conn);
        }
    }

    #[test]
    fn r2_restarts_after_link_loss_and_uses_the_connection_that_raised() {
        let mut cfg = EngineConfig::default();
        cfg.rules.r2.wait_s = 1;
        let mut engine = Engine::new(cfg);
        apply(&mut engine, 0, added(1));
        apply(&mut engine, 100, link(1, true));
        apply(&mut engine, 1_101, serial_known(1));
        apply(&mut engine, 1_200, link(1, false));
        apply(&mut engine, 1_300, link(1, true));
        apply(&mut engine, 1_400, link(1, true));
        apply(&mut engine, 2_400, added(1));
        apply(&mut engine, 2_500, link(1, true));
        engine.advance_to(LogTime(3_501));

        let output = detections(&mut engine);
        assert_eq!(
            steps(&output),
            [
                (RuleId::R2, 1, Step::Raised, 1_100),
                (RuleId::R2, 1, Step::Ended, 1_200),
                (RuleId::R2, 1, Step::Raised, 2_300),
                (RuleId::R2, 1, Step::Ended, 2_400),
                (RuleId::R2, 1, Step::Raised, 3_500),
            ]
        );
        assert_eq!(output[0].episode, output[1].episode);
        assert_eq!(output[2].episode, output[3].episode);
        assert_ne!(output[0].episode, output[2].episode);
        assert_eq!(output[0].serial, None);
        assert_eq!(output[1].serial, None);
        assert_eq!(output[2].serial, Some(Serial("SYNTHETIC-01".into())));
        assert_eq!(output[2].serial, output[3].serial);
        assert_eq!(output[0].conn, Some(ConnId(1)));
        assert_eq!(output[0].conn, output[3].conn);
        assert_eq!(output[4].conn, Some(ConnId(2)));
    }

    #[test]
    fn r3_host_wait_starts_at_the_command_and_is_not_reset_by_a_repeat() {
        for delay in [0, 1] {
            let mut cfg = EngineConfig::default();
            cfg.rules.r3.wait_s = 2;
            let mut engine = Engine::new(cfg);
            apply(&mut engine, 0, added(1));
            apply(&mut engine, 100, map_ready(1, true));
            engine.advance_to(LogTime(2_500));
            assert!(detections(&mut engine).is_empty());

            apply(&mut engine, 3_000, host_assigned(1));
            apply(&mut engine, 3_500, host_assigned(1));
            apply(&mut engine, 4_000, serial_known(1));
            engine.advance_to(LogTime(5_000));
            assert!(detections(&mut engine).is_empty());

            apply(&mut engine, 5_000 + delay, pose(1, PoseStatus::Ok));
            engine.advance_to(LogTime(5_500));
            let output = detections(&mut engine);
            if delay == 0 {
                assert!(output.is_empty());
            } else {
                assert_eq!(
                    steps(&output),
                    [
                        (RuleId::R3, 1, Step::Raised, 5_000),
                        (RuleId::R3, 1, Step::Resolved, 5_001),
                    ]
                );
                assert_eq!(output[0].conn, Some(ConnId(1)));
                assert_eq!(output[0].serial, Some(Serial("SYNTHETIC-01".into())));
            }
        }
    }

    #[test]
    fn r3_default_host_change_cancels_a_pending_wait_or_ends_a_raised_episode() {
        for handoff in [1_100, 1_101] {
            let mut cfg = EngineConfig::default();
            cfg.rules.r3.wait_s = 1;
            let mut engine = Engine::new(cfg);
            apply(&mut engine, 0, added(1));
            apply(&mut engine, 0, added(2));
            apply(&mut engine, 100, host_assigned(1));
            apply(&mut engine, handoff, host_assigned(2));
            engine.advance_to(LogTime(2_200));

            let mut expected = Vec::new();
            if handoff > 1_100 {
                expected.extend([
                    (RuleId::R3, 1, Step::Raised, 1_100),
                    (RuleId::R3, 1, Step::Ended, handoff),
                ]);
            }
            expected.push((RuleId::R3, 2, Step::Raised, handoff + 1_000));
            assert_eq!(steps(&detections(&mut engine)), expected);
        }
    }

    #[test]
    fn r3_client_setting_applies_only_to_client_connections_with_a_known_start() {
        for clients in [false, true] {
            let mut cfg = EngineConfig::default();
            cfg.rules.r3.wait_s = 1;
            cfg.rules.r3.clients = clients;
            let mut engine = Engine::new(cfg);
            apply(&mut engine, 0, added(1));
            apply(&mut engine, 100, link(1, true));
            apply(&mut engine, 100, link(2, true)); // Inferred client.
            apply(&mut engine, 200, map_ready(1, false));
            apply(&mut engine, 200, map_ready(2, false));
            apply(&mut engine, 300, host_assigned(3)); // Inferred host.
            engine.advance_to(LogTime(2_000));

            let output = detections(&mut engine);
            if clients {
                assert_eq!(steps(&output), [(RuleId::R3, 1, Step::Raised, 1_200)]);
            } else {
                assert!(output.is_empty());
            }
        }
    }

    #[test]
    fn r5_is_opt_in_and_only_tracks_loss_after_the_first_pose() {
        for (enabled, delay) in [(false, 1), (true, 0), (true, 1)] {
            let mut cfg = EngineConfig::default();
            cfg.rules.r3.wait_s = 2;
            cfg.rules.r5.wait_s = 2;
            cfg.rules.r5.enabled = enabled;
            let mut engine = Engine::new(cfg);
            apply(&mut engine, 0, added(1));
            apply(&mut engine, 10, host_assigned(1));
            apply(&mut engine, 500, pose(1, PoseStatus::Ok));
            apply(&mut engine, 1_000, pose(1, PoseStatus::Lost));
            apply(&mut engine, 1_500, pose(1, PoseStatus::RecentlyLost));
            apply(&mut engine, 2_000, serial_known(1));
            engine.advance_to(LogTime(3_000));
            assert!(detections(&mut engine).is_empty());

            apply(&mut engine, 3_000 + delay, pose(1, PoseStatus::Ok));
            engine.advance_to(LogTime(4_000));
            let output = detections(&mut engine);
            if enabled && delay == 1 {
                assert_eq!(
                    steps(&output),
                    [
                        (RuleId::R5, 1, Step::Raised, 3_000),
                        (RuleId::R5, 1, Step::Resolved, 3_001),
                    ]
                );
                assert_eq!(output[0].conn, Some(ConnId(1)));
                assert_eq!(output[0].serial, Some(Serial("SYNTHETIC-01".into())));
            } else {
                assert!(output.is_empty());
            }
        }
    }

    #[test]
    fn reconnect_cancels_pending_r2_and_r3_and_ends_raised_r5() {
        let mut cfg = EngineConfig::default();
        cfg.rules.r5.wait_s = 1;
        cfg.rules.r5.enabled = true;
        let mut engine = Engine::new(cfg);
        apply(&mut engine, 0, added(1));
        apply(&mut engine, 0, added(2));
        apply(&mut engine, 0, added(3));
        apply(&mut engine, 100, host_assigned(1));
        apply(&mut engine, 110, link(2, true));
        apply(&mut engine, 120, pose(3, PoseStatus::Ok));
        apply(&mut engine, 130, pose(3, PoseStatus::Lost));
        apply(&mut engine, 1_200, added(1));
        apply(&mut engine, 1_200, added(2));
        apply(&mut engine, 1_200, added(3));
        engine.advance_to(LogTime(70_000));

        let output = detections(&mut engine);
        assert_eq!(
            steps(&output),
            [
                (RuleId::R5, 3, Step::Raised, 1_130),
                (RuleId::R5, 3, Step::Ended, 1_200),
            ]
        );
        assert_eq!(output[0].episode, output[1].episode);
        assert_eq!(output[1].conn, Some(ConnId(3)));
    }
}
