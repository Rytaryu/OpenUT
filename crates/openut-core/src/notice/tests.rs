use crate::state::State;

use super::*;
use RuleId::{R1, R2, R3, R4, R5};
use Step::{Ended, Escalated, Raised, Resolved};

fn scheduler(min_gap_s: u64) -> NoticeScheduler {
    NoticeScheduler::new(NoticeConfig {
        min_gap_s,
        ..NoticeConfig::default()
    })
}

fn snapshot(at: i64) -> Snapshot {
    State::default().snapshot(LogTime(at))
}

fn apply(scheduler: &mut NoticeScheduler, rule: RuleId, episode: u64, step: Step, at: i64) {
    apply_to(scheduler, rule, episode, step, at, (Some(1), None));
}

fn apply_to(
    scheduler: &mut NoticeScheduler,
    rule: RuleId,
    episode: u64,
    step: Step,
    at: i64,
    (tracker, serial): (Option<u16>, Option<&str>),
) {
    scheduler.advance_to(LogTime(at));
    scheduler.apply(
        &Detection {
            rule,
            tracker: tracker.map(TrackerKey),
            episode,
            serial: serial.map(|serial| Serial(serial.to_owned())),
            conn: Some(ConnId(episode)),
            host_wifi_queried: None,
            step,
            at: LogTime(at),
        },
        &snapshot(at),
    );
}

fn take_pushes(scheduler: &mut NoticeScheduler) -> Vec<Notice> {
    scheduler
        .drain()
        .into_iter()
        .filter(|notice| notice.push)
        .collect()
}

#[test]
fn delivery_table_and_reconnected_display_lifetime() {
    for rule in [R1, R2, R3, R4, R5] {
        for terminal in [Resolved, Ended] {
            let mut scheduler = scheduler(0);
            apply(&mut scheduler, rule, 1, Raised, 0);
            let mut delivered = scheduler.drain();
            if rule == R1 {
                apply(&mut scheduler, rule, 1, Escalated, 180_000);
                delivered.extend(scheduler.drain());
            }
            apply(&mut scheduler, rule, 1, terminal, 181_000);
            delivered.extend(scheduler.drain());
            assert_eq!(
                delivered
                    .into_iter()
                    .map(|notice| (notice.step, notice.push))
                    .collect::<Vec<_>>(),
                if rule == R1 {
                    vec![
                        (Raised, true),
                        (Escalated, true),
                        (terminal, terminal == Resolved),
                    ]
                } else {
                    vec![(Raised, true), (terminal, false)]
                }
            );
            if rule == R4 && terminal == Resolved {
                assert_eq!(scheduler.live()[0].step, Resolved);
                assert_eq!(scheduler.next_deadline(), Some(LogTime(211_000)));
                scheduler.advance_to(LogTime(210_999));
                assert_eq!(scheduler.live().len(), 1);
                scheduler.advance_to(LogTime(211_000));
            }
            assert!(scheduler.live().is_empty());
        }
    }
}

#[test]
fn cooldown_starts_after_r2_r3_r5_end_and_elapsed_boundary_is_allowed() {
    for rule in [R2, R3, R5] {
        for elapsed in [299_999, 300_000] {
            let mut scheduler = scheduler(0);
            apply(&mut scheduler, rule, 1, Raised, 0);
            assert_eq!(take_pushes(&mut scheduler).len(), 1);
            apply(&mut scheduler, rule, 1, Ended, 100);
            apply(&mut scheduler, rule, 2, Raised, 100 + elapsed);
            let pushes = take_pushes(&mut scheduler);
            assert_eq!(pushes.len(), usize::from(elapsed == 300_000));
            assert_eq!(scheduler.live()[0].episode, 2);
        }
    }

    let mut scheduler = scheduler(10);
    for (episode, at) in [(1, 0), (2, 180_000), (3, 360_000)] {
        apply(&mut scheduler, R1, episode, Raised, at);
        assert_eq!(take_pushes(&mut scheduler).len(), 1);
        apply(&mut scheduler, R1, episode, Ended, at + 1);
    }
    assert!(take_pushes(&mut scheduler).is_empty());
}

#[test]
fn r4_repetitions_match_serial_if_both_are_known_and_slot_otherwise() {
    for (first, next, expected_push) in [
        ((Some(2), Some("S")), (Some(4), Some("S")), false),
        ((Some(2), Some("S")), (Some(2), Some("T")), true),
        ((Some(2), Some("S")), (Some(2), None), false),
        ((Some(2), None), (Some(2), None), false),
        ((Some(2), None), (Some(4), None), true),
    ] {
        let mut scheduler = scheduler(10);
        apply_to(&mut scheduler, R4, 1, Raised, 0, first);
        assert_eq!(take_pushes(&mut scheduler).len(), 1);
        apply_to(&mut scheduler, R4, 1, Resolved, 20_000, first);
        apply_to(&mut scheduler, R4, 2, Raised, 180_000, next);
        assert_eq!(!take_pushes(&mut scheduler).is_empty(), expected_push);
        assert_eq!(
            scheduler.live()[0].repetitions,
            if expected_push { 1 } else { 2 }
        );
    }
}

#[test]
fn gap_batches_waiting_notices_and_leaves_live_items_ungrouped() {
    let mut scheduler = scheduler(10);
    apply(&mut scheduler, R1, 1, Raised, 0);
    assert_eq!(take_pushes(&mut scheduler).len(), 1);
    apply(&mut scheduler, R2, 2, Raised, 2_000);
    apply(&mut scheduler, R5, 3, Raised, 3_000);
    apply(&mut scheduler, R4, 4, Raised, 9_999);
    scheduler.advance_to(LogTime(10_000));
    assert!(take_pushes(&mut scheduler).is_empty());
    assert_eq!(scheduler.next_deadline(), Some(LogTime(10_000)));
    scheduler.advance_to(LogTime(10_001));
    let [notice] = take_pushes(&mut scheduler)
        .try_into()
        .expect("one bundled push");
    assert_eq!((notice.rule, notice.at), (R4, LogTime(10_000)));
    assert_eq!(notice.other_count, 2);
    assert!(
        scheduler
            .live()
            .iter()
            .all(|notice| notice.other_count == 0)
    );
}

#[test]
fn waiting_notices_disappear_if_fixed_at_the_deadline_or_ended_before_it() {
    let mut scheduler = scheduler(10);
    apply(&mut scheduler, R1, 1, Raised, 0);
    assert_eq!(take_pushes(&mut scheduler).len(), 1);
    apply(&mut scheduler, R2, 2, Raised, 3_000);
    apply(&mut scheduler, R3, 3, Raised, 4_000);
    apply(&mut scheduler, R3, 3, Ended, 9_999);
    apply(&mut scheduler, R2, 2, Resolved, 10_000);
    scheduler.advance_to(LogTime(10_001));
    assert!(take_pushes(&mut scheduler).is_empty());
    assert_eq!(scheduler.live().len(), 1);
}

#[test]
fn a_disconnect_prepared_at_52_seconds_is_withdrawn_before_drain() {
    for terminal in [Ended, Resolved] {
        let mut scheduler = scheduler(10);
        apply(&mut scheduler, R1, 1, Raised, 42_000);
        assert_eq!(take_pushes(&mut scheduler)[0].rule, R1);
        apply(&mut scheduler, R4, 2, Raised, 45_000);
        scheduler.advance_to(LogTime(52_001));
        assert_eq!(scheduler.last_push(), Some(LogTime(52_000)));

        apply(&mut scheduler, R4, 2, terminal, 52_001);
        let notices = scheduler.drain();
        assert!(notices.iter().all(|notice| !notice.push));
        assert!(notices.iter().any(|notice| notice.step == terminal));
        assert_eq!(scheduler.last_push(), Some(LogTime(42_000)));

        // The withdrawn disconnect starts neither the cadence nor the R4 cooldown.
        apply(&mut scheduler, R4, 3, Raised, 53_000);
        let [fresh] = take_pushes(&mut scheduler).try_into().expect("new R4 push");
        assert_eq!(
            (fresh.episode, fresh.at, fresh.repetitions),
            (3, LogTime(53_000), 1)
        );
    }
}

#[test]
fn an_overlapping_unknown_disconnect_uses_only_a_delivered_cooldown() {
    for (restore_known, drain_at) in [(true, 55_000), (false, 55_000), (false, 62_001)] {
        let mut scheduler = scheduler(10);
        apply(&mut scheduler, R1, 1, Raised, 42_000);
        assert_eq!(take_pushes(&mut scheduler).len(), 1);
        apply_to(
            &mut scheduler,
            R4,
            2,
            Raised,
            45_000,
            (Some(1), Some("KNOWN")),
        );
        apply_to(&mut scheduler, R4, 3, Raised, 54_000, (Some(1), None));
        if restore_known {
            apply_to(
                &mut scheduler,
                R4,
                2,
                Resolved,
                55_000,
                (Some(1), Some("KNOWN")),
            );
        }
        scheduler.advance_to(LogTime(drain_at));

        let output = scheduler.drain();
        let pushes: Vec<_> = output.iter().filter(|notice| notice.push).collect();
        assert_eq!(pushes.len(), 1);
        let second = scheduler
            .live()
            .into_iter()
            .find(|notice| notice.episode == 3)
            .unwrap();
        if restore_known {
            assert_eq!((pushes[0].episode, pushes[0].at), (3, LogTime(54_000)));
            assert_eq!(second.repetitions, 1);
        } else {
            assert_eq!((pushes[0].episode, pushes[0].at), (2, LogTime(52_000)));
            assert_eq!(second.repetitions, 2);
            assert!(
                output
                    .iter()
                    .any(|notice| notice.episode == 3 && !notice.push && notice.repetitions == 2)
            );
        }
        scheduler.advance_to(LogTime(80_000));
        assert!(take_pushes(&mut scheduler).is_empty());
    }
}

#[test]
fn an_already_prepared_batch_promotes_its_next_live_candidate() {
    let mut scheduler = scheduler(10);
    apply(&mut scheduler, R1, 1, Raised, 42_000);
    assert_eq!(take_pushes(&mut scheduler).len(), 1);
    apply(&mut scheduler, R4, 2, Raised, 45_000);
    apply(&mut scheduler, R2, 3, Raised, 46_000);
    apply(&mut scheduler, R3, 4, Raised, 47_000);
    apply(&mut scheduler, R5, 5, Raised, 48_000);
    scheduler.advance_to(LogTime(52_001));
    apply(&mut scheduler, R2, 3, Resolved, 52_001);
    apply(&mut scheduler, R4, 2, Ended, 52_001);

    let [winner] = take_pushes(&mut scheduler)
        .try_into()
        .expect("remaining batch");
    assert_eq!(
        (winner.rule, winner.episode, winner.at),
        (R3, 4, LogTime(52_000))
    );
    assert_eq!(winner.other_count, 1);
    assert_eq!(
        scheduler
            .live()
            .iter()
            .map(|notice| notice.rule)
            .collect::<Vec<_>>(),
        [R1, R3, R5]
    );
}

#[test]
fn cancelling_an_r2_push_preserves_end_based_cooldown() {
    let mut scheduler = scheduler(10);
    apply(&mut scheduler, R2, 1, Raised, 0);
    apply(&mut scheduler, R2, 1, Resolved, 100);
    assert!(take_pushes(&mut scheduler).is_empty());

    apply(&mut scheduler, R2, 2, Raised, 101);
    assert!(take_pushes(&mut scheduler).is_empty());
}

#[test]
fn catch_up_releases_only_live_notices_and_groups_by_priority() {
    for gap in [0, 10] {
        let mut scheduler = scheduler(gap);
        scheduler.set_catching_up(true, LogTime(0));
        apply(&mut scheduler, R1, 1, Raised, 100);
        apply(&mut scheduler, R1, 1, Resolved, 200);
        apply(&mut scheduler, R5, 2, Raised, 300);
        apply(&mut scheduler, R3, 3, Raised, 400);
        assert!(take_pushes(&mut scheduler).is_empty());
        scheduler.set_catching_up(false, LogTime(1_000));
        let notices = take_pushes(&mut scheduler);
        let rules: Vec<_> = notices.iter().map(|notice| notice.rule).collect();
        if gap == 0 {
            assert_eq!(rules, [R3, R5]);
        } else {
            assert_eq!(rules, [R3]);
        }
        assert_eq!(notices[0].at, LogTime(1_000));
        assert_eq!(notices[0].other_count, usize::from(gap != 0));
    }
}

#[test]
fn rebuild_matches_delivered_stage_by_rule_and_subject_not_episode_or_connection() {
    for was_escalated in [false, true] {
        let mut scheduler = scheduler(0);
        apply_to(&mut scheduler, R1, 1, Raised, 0, (Some(1), Some("S")));
        if was_escalated {
            apply_to(&mut scheduler, R1, 1, Escalated, 10, (Some(1), Some("S")));
        }
        assert_eq!(
            take_pushes(&mut scheduler).len(),
            if was_escalated { 2 } else { 1 }
        );
        scheduler.rebuild();
        scheduler.set_catching_up(true, LogTime(0));
        apply_to(&mut scheduler, R1, 2, Raised, 20, (Some(4), Some("S")));
        scheduler.set_catching_up(false, LogTime(20));
        assert!(take_pushes(&mut scheduler).is_empty());
        apply_to(&mut scheduler, R1, 2, Escalated, 30, (Some(4), Some("S")));
        assert_eq!(
            take_pushes(&mut scheduler).len(),
            if was_escalated { 0 } else { 1 }
        );

        scheduler.rebuild();
        scheduler.set_catching_up(true, LogTime(0));
        apply_to(&mut scheduler, R1, 2, Raised, 40, (Some(4), Some("T")));
        scheduler.set_catching_up(false, LogTime(40));
        assert_eq!(
            take_pushes(&mut scheduler)[0].serial,
            Some(Serial("T".into()))
        );
        assert_eq!(scheduler.live().len(), 1);
    }
}

#[test]
fn stopping_rolls_back_undrained_pushes_and_cancels_the_waiting_batch() {
    let mut scheduler = scheduler(10);
    apply(&mut scheduler, R4, 1, Raised, 0);
    apply(&mut scheduler, R2, 2, Raised, 1_000);
    scheduler.set_hub_down(true);
    assert_eq!(scheduler.next_deadline(), None);
    let notices = scheduler.drain();
    assert_eq!(notices.len(), 2);
    assert!(notices.iter().all(|notice| !notice.push));
    apply(&mut scheduler, R4, 1, Ended, 2_000);
    apply(&mut scheduler, R2, 2, Ended, 2_000);
    scheduler.set_hub_down(false);
    apply(&mut scheduler, R4, 3, Raised, 3_000);
    let pushes = take_pushes(&mut scheduler);
    assert_eq!((pushes[0].rule, pushes[0].at), (R4, LogTime(3_000)));
    apply(&mut scheduler, R2, 4, Raised, 13_000);
    assert_eq!(take_pushes(&mut scheduler)[0].rule, R2);
}

#[test]
fn stopping_preserves_delivered_cooldown_but_never_replays_muted_notices() {
    let mut scheduler = scheduler(10);
    apply(&mut scheduler, R4, 1, Raised, 0);
    assert_eq!(take_pushes(&mut scheduler).len(), 1);
    scheduler.set_hub_down(true);
    scheduler.set_catching_up(true, LogTime(1_000));
    apply(&mut scheduler, R2, 2, Raised, 1_000);
    apply(&mut scheduler, R4, 1, Resolved, 2_000);
    apply(&mut scheduler, R5, 3, Raised, 4_000);
    scheduler.set_catching_up(false, LogTime(10_000));
    scheduler.set_hub_down(false);
    assert!(take_pushes(&mut scheduler).is_empty());
    assert_eq!(scheduler.live().len(), 3);
    apply(&mut scheduler, R2, 2, Ended, 30_000);
    apply(&mut scheduler, R2, 4, Raised, 31_000);
    apply(&mut scheduler, R4, 5, Raised, 50_000);
    let pushes = take_pushes(&mut scheduler);
    assert_eq!(pushes.len(), 1);
    assert_eq!((pushes[0].rule, pushes[0].episode), (R2, 4));
}

#[test]
fn a_hub_restart_during_catch_up_only_releases_new_active_notices() {
    let mut scheduler = scheduler(0);
    scheduler.set_hub_down(true);
    scheduler.set_catching_up(true, LogTime(0));
    apply(&mut scheduler, R4, 1, Raised, 1_000);
    scheduler.set_hub_down(false);
    apply(&mut scheduler, R2, 2, Raised, 50_000);
    scheduler.set_catching_up(false, LogTime(50_000));

    let [notice] = take_pushes(&mut scheduler)
        .try_into()
        .expect("one fresh push");
    assert_eq!(notice.rule, R2);
    assert_eq!(scheduler.live().len(), 2);
}
