//! Connects the pure calibration flow to live SteamVR, the bridge, and storage.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use openut_space::calib::{CalibrationConfig, TimedSample};

use crate::calib_file;
use crate::calib_flow::{CalibrationFlow, CalibrationStatus};
use crate::config::RunConfig;
use crate::steamvr::{
    self,
    board::{BoardAction, Dashboard},
    client::{Client, ClientEvent},
};

use super::pipeline::Pipeline;
use super::windows::AppLog;

const POLL_INTERVAL: Duration = Duration::from_millis(33);

pub(super) struct CalibrationRuntime {
    origin: Instant,
    path: Option<PathBuf>,
    flow: CalibrationFlow,
    client: Option<Client>,
    board: Option<Dashboard>,
    connection_epoch: Option<u64>,
    sampling: bool,
    next_poll: Duration,
    last_notice: Option<&'static str>,
}

impl CalibrationRuntime {
    pub(super) fn start(origin: Instant, config: RunConfig, log: &mut AppLog) -> Self {
        let path = std::env::var_os("APPDATA")
            .map(|root| PathBuf::from(root).join("OpenUT").join("calibration.json"));
        let saved = path
            .as_deref()
            .and_then(|path| match calib_file::read(path) {
                Ok(saved) => saved,
                Err(error) => {
                    log.write(format_args!(
                        "キャリブレーションを読み取れません（{}）。",
                        error.kind()
                    ));
                    None
                }
            });
        let client = match steamvr::start(origin) {
            Ok(client) => Some(client),
            Err(error) => {
                log.write(format_args!(
                    "SteamVR の接続を監視できません（{}）。",
                    error.kind()
                ));
                None
            }
        };
        let board = client.as_ref().and_then(|_| match Dashboard::new() {
            Ok(board) => Some(board),
            Err(error) => {
                log.write(format_args!(
                    "SteamVR のダッシュボードを表示できません（{error}）。"
                ));
                None
            }
        });
        let flow = CalibrationFlow::new(
            CalibrationConfig {
                fix_scale: config.calibration_fix_scale,
                ..CalibrationConfig::default()
            },
            config.axis_map,
            saved,
        );
        Self {
            origin,
            path,
            flow,
            client,
            board,
            connection_epoch: None,
            sampling: false,
            next_poll: Duration::ZERO,
            last_notice: None,
        }
    }

    pub(super) fn apply_to(&self, pipeline: &mut Pipeline) {
        let (axis_map, calibration) = self.flow.active();
        pipeline.set_calibration(axis_map, calibration);
    }

    pub(super) fn add_tracker(&mut self, tracker: u32, sample: TimedSample) {
        self.flow.add_tracker(tracker, sample);
    }

    pub(super) fn poll(
        &mut self,
        pipeline: &mut Pipeline,
        out: &mut impl Write,
        log: &mut AppLog,
    ) -> io::Result<()> {
        let now = self.origin.elapsed();
        if now < self.next_poll {
            return Ok(());
        }
        self.next_poll = now.saturating_add(POLL_INTERVAL);
        if let Some(client) = &self.client {
            let state = client.state();
            let epoch = state.connected.then_some(state.connection_epoch);
            if epoch != self.connection_epoch {
                self.flow.set_fingerprint(None);
                self.connection_epoch = epoch;
            }
            self.flow.set_fingerprint(state.fingerprint);
            set_sampling(client, &mut self.sampling, self.flow.is_collecting());
            for event in client.events() {
                let ClientEvent::Mouse(event) = event else {
                    continue;
                };
                let action = self
                    .board
                    .as_ref()
                    .and_then(|board| board.press(state, event));
                match action {
                    Some(BoardAction::Start) => {
                        self.flow.start(now);
                    }
                    Some(BoardAction::Cancel) => self.flow.cancel(),
                    None => continue,
                }
                set_sampling(client, &mut self.sampling, self.flow.is_collecting());
            }
            if self.sampling {
                for batch in client
                    .samples()
                    .filter(|batch| Some(batch.connection_epoch) == epoch)
                {
                    for controller in batch.controllers {
                        self.flow
                            .add_controller(controller.device, controller.sample);
                    }
                }
            }
            let latest = client.state();
            if latest.connection_epoch != state.connection_epoch
                || latest.connected != state.connected
            {
                self.flow.set_fingerprint(None);
            } else {
                self.flow.set_fingerprint(latest.fingerprint);
            }
            let unix_s = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let candidate = self.flow.tick(self.origin.elapsed(), unix_s);
            set_sampling(client, &mut self.sampling, self.flow.is_collecting());
            if let Some(record) = candidate {
                let stored = self
                    .path
                    .as_deref()
                    .map(|path| calib_file::write(path, &record));
                self.flow.finish_save(matches!(&stored, Some(Ok(()))));
                match stored {
                    Some(Ok(())) => {
                        let (axis_map, calibration) = self.flow.active();
                        pipeline.set_calibration(axis_map, calibration);
                        log.write(format_args!("キャリブレーションを保存しました。位置の残差 {:.1} mm、サンプル {} 件。",
                            record.position_rms_m * 1_000.0, record.sample_count));
                    }
                    Some(Err(error)) => {
                        writeln!(out, "キャリブレーションの結果を保存できませんでした。")?;
                        log.write(format_args!(
                            "キャリブレーションを保存できません（{}）。",
                            error.kind()
                        ));
                    }
                    None => {
                        writeln!(out, "キャリブレーションの保存先が見つかりません。")?;
                        log.write(format_args!("APPDATA を取得できません。"));
                    }
                }
            }
            if let Some(board) = &mut self.board
                && let Err(error) = board.update(client, latest, &self.flow.view())
            {
                log.write(format_args!(
                    "SteamVR のダッシュボードを表示できません（{error}）。"
                ));
            }
        }
        let view = self.flow.view();
        let notice = if view.needs_recalibration {
            Some("SteamVR の空間が変わりました。キャリブレーションをやり直してください。")
        } else {
            match view.status {
                CalibrationStatus::Needed => Some("キャリブレーションが必要です。"),
                CalibrationStatus::Completed { .. } => Some("キャリブレーションが完了しました。"),
                _ => None,
            }
        };
        if let Some(notice) = notice
            && self.last_notice != Some(notice)
        {
            writeln!(out, "{notice}")?;
            log.write(format_args!("{notice}"));
        }
        self.last_notice = notice;
        Ok(())
    }
}

impl Drop for CalibrationRuntime {
    fn drop(&mut self) {
        if let Some(client) = &mut self.client {
            if let Some(board) = &mut self.board {
                board.shutdown(client);
            }
            client.shutdown();
        }
    }
}

fn set_sampling(client: &Client, sampling: &mut bool, enabled: bool) {
    if *sampling != enabled {
        client.set_calibration(enabled);
        *sampling = enabled;
    }
}
