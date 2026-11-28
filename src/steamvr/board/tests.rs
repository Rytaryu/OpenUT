use std::sync::mpsc::{SyncSender, sync_channel};

use crate::calib_flow::{CalibrationFailure, RotationProgress};

use super::*;

const HANDLES: OverlayHandles = OverlayHandles {
    dashboard: 41,
    thumbnail: 42,
};

fn connected(epoch: u64) -> ClientState {
    ClientState {
        connected: true,
        connection_epoch: epoch,
        fingerprint: None,
    }
}

fn view(status: CalibrationStatus) -> CalibrationView {
    CalibrationView {
        status,
        has_calibration: false,
        needs_recalibration: false,
    }
}

fn mouse() -> MouseEvent {
    // OpenVR reports bottom-left coordinates, while the rendered button starts at y=304.
    MouseEvent {
        handle: HANDLES.dashboard,
        kind: MouseEventKind::ButtonDown,
        button: LEFT_BUTTON,
        x: 60.0,
        y: pixels::HEIGHT as f32 - 320.0,
    }
}

#[derive(Default)]
struct MockOverlay {
    attempts: Vec<(u64, OverlayCommand)>,
    reply: Option<SyncSender<OverlayResult>>,
    full: bool,
}

impl MockOverlay {
    fn tick(
        &mut self,
        board: &mut BoardOverlay,
        state: ClientState,
        view: CalibrationView,
    ) -> Result<(), BoardError> {
        board.update(
            state,
            view,
            |epoch, command| {
                self.attempts.push((epoch, command.clone()));
                if self.full {
                    return Err(command);
                }
                let (send, receive) = sync_channel(1);
                self.reply = Some(send);
                Ok(receive)
            },
            |view| if view.is_some() { vec![2] } else { vec![1] },
        )
    }

    fn respond(&mut self, result: OverlayResult) {
        self.reply
            .take()
            .expect("a submitted command")
            .send(result)
            .unwrap();
    }

    fn last(&self) -> &(u64, OverlayCommand) {
        self.attempts.last().unwrap()
    }

    fn show(&mut self, board: &mut BoardOverlay, state: ClientState, view: CalibrationView) {
        self.tick(board, state, view).unwrap();
        self.respond(Ok(Some(HANDLES)));
        for _ in 0..4 {
            self.tick(board, state, view).unwrap();
            self.respond(Ok(None));
        }
        self.tick(board, state, view).unwrap();
    }
}

#[test]
fn configures_one_overlay_and_only_accepts_primary_presses_on_its_visible_button() {
    let mut board = BoardOverlay::default();
    let mut vr = MockOverlay::default();
    let state = connected(3);
    let needed = view(CalibrationStatus::Needed);
    vr.tick(&mut board, ClientState::default(), needed).unwrap();
    assert!(vr.attempts.is_empty());
    assert_eq!(board.press(state, mouse()), None);

    vr.show(&mut board, state, needed);
    let commands: Vec<_> = vr
        .attempts
        .iter()
        .map(|(epoch, command)| {
            assert_eq!(*epoch, 3);
            command
        })
        .collect();
    assert!(matches!(
        commands.as_slice(),
        [
            OverlayCommand::CreateDashboard { .. },
            OverlayCommand::SetWidth { handle: 41, .. },
            OverlayCommand::SetMouseScale { handle: 41, .. },
            OverlayCommand::SetImage { handle: 42, .. },
            OverlayCommand::SetImage { handle: 41, .. },
        ]
    ));
    assert_eq!(
        commands[2],
        &OverlayCommand::SetMouseScale {
            handle: 41,
            width: pixels::WIDTH as f32,
            height: pixels::HEIGHT as f32
        }
    );
    assert_eq!(board.press(state, mouse()), Some(BoardAction::Start));
    for other in [
        MouseEvent {
            handle: HANDLES.thumbnail,
            ..mouse()
        },
        MouseEvent {
            kind: MouseEventKind::Move,
            ..mouse()
        },
        MouseEvent {
            kind: MouseEventKind::ButtonUp,
            ..mouse()
        },
        MouseEvent {
            button: 2,
            ..mouse()
        },
        MouseEvent { x: 10.0, ..mouse() },
        MouseEvent {
            y: 320.0,
            ..mouse()
        },
        MouseEvent {
            x: f32::NAN,
            ..mouse()
        },
    ] {
        assert_eq!(board.press(state, other), None);
    }
    assert_eq!(board.press(connected(4), mouse()), None);

    let collecting = view(CalibrationStatus::Collecting {
        motion: RotationProgress::OneAxis,
    });
    vr.tick(&mut board, state, collecting).unwrap();
    assert!(matches!(
        vr.last().1,
        OverlayCommand::SetImage { handle: 41, .. }
    ));
    vr.respond(Ok(None));
    vr.tick(&mut board, state, collecting).unwrap();
    assert_eq!(board.press(state, mouse()), Some(BoardAction::Cancel));
    assert_eq!(vr.attempts.len(), 6, "unchanged view does not upload again");
}

#[test]
fn reconnection_recreates_the_board_without_sending_an_old_handle_to_the_new_epoch() {
    let mut board = BoardOverlay::default();
    let mut vr = MockOverlay::default();
    let needed = view(CalibrationStatus::Needed);
    vr.show(&mut board, connected(1), needed);
    vr.tick(&mut board, connected(2), needed).unwrap();
    assert!(matches!(
        vr.last(),
        (2, OverlayCommand::CreateDashboard { .. })
    ));
    assert_eq!(board.press(connected(2), mouse()), None);
    vr.tick(&mut board, connected(2), needed).unwrap();
    assert_eq!(vr.attempts.len(), 6, "waits for the in-flight request");
}

#[test]
fn partially_created_board_is_removed_and_failure_is_reported_once_per_epoch() {
    let mut board = BoardOverlay::default();
    let mut vr = MockOverlay::default();
    let state = connected(1);
    let needed = view(CalibrationStatus::Needed);
    vr.tick(&mut board, state, needed).unwrap();
    vr.respond(Ok(Some(HANDLES)));
    vr.tick(&mut board, state, needed).unwrap();
    vr.respond(Err("fixture error".into()));
    vr.full = true;
    assert_eq!(
        vr.tick(&mut board, state, needed),
        Err(BoardError::Overlay("fixture error".into()))
    );
    assert_eq!(
        vr.last(),
        &(
            1,
            OverlayCommand::Destroy {
                handle: HANDLES.dashboard
            }
        )
    );
    vr.full = false;
    vr.tick(&mut board, state, needed).unwrap();
    assert_eq!(
        vr.last(),
        &(
            1,
            OverlayCommand::Destroy {
                handle: HANDLES.dashboard
            }
        )
    );
    vr.tick(&mut board, state, needed).unwrap();
    assert_eq!(vr.attempts.len(), 4);
    assert_eq!(board.press(state, mouse()), None);
    vr.tick(&mut board, connected(2), needed).unwrap();
    assert!(matches!(
        vr.last(),
        (2, OverlayCommand::CreateDashboard { .. })
    ));
}

#[test]
fn reports_an_unsaved_result_and_the_previous_calibration_when_present() {
    let save_failed = view(CalibrationStatus::Failed(CalibrationFailure::SaveFailed));
    let heading = "キャリブレーションに失敗しました";
    assert_eq!(
        pixels::status_text(save_failed),
        (heading, "結果を保存できませんでした。")
    );
    assert_eq!(
        pixels::status_text(CalibrationView {
            has_calibration: true,
            ..save_failed
        }),
        (
            heading,
            "結果を保存できませんでした。\n前回のキャリブレーションを使っています。"
        )
    );
}

#[test]
fn rasterizes_opaque_rgba_when_a_local_japanese_font_is_available() {
    use std::{fs, path::Path};

    assert!(pixels::font_in(Path::new("this-directory-does-not-contain-windows-fonts")).is_none());
    let font = pixels::system_font().or_else(|| {
        let bytes = fs::read("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc").ok()?;
        pixels::load_font(&bytes, "Noto Sans CJK JP")
    });
    let Some(font) = font else {
        eprintln!("skipped raster test because no local Japanese font is available");
        return;
    };
    let initial = pixels::board(&font, view(CalibrationStatus::Needed));
    let collecting = pixels::board(
        &font,
        view(CalibrationStatus::Collecting {
            motion: RotationProgress::Ready,
        }),
    );
    let thumbnail = pixels::thumbnail(&font);
    assert_eq!(initial.len(), (pixels::WIDTH * pixels::HEIGHT * 4) as usize);
    assert_eq!(collecting.len(), initial.len());
    assert_eq!(
        thumbnail.len(),
        (pixels::THUMBNAIL * pixels::THUMBNAIL * 4) as usize
    );
    assert_ne!(initial, collecting);
    let save_failed = view(CalibrationStatus::Failed(CalibrationFailure::SaveFailed));
    let first_save = pixels::board(&font, save_failed);
    let previous = pixels::board(
        &font,
        CalibrationView {
            has_calibration: true,
            ..save_failed
        },
    );
    assert_ne!(first_save, previous);
    let completed = pixels::board(
        &font,
        CalibrationView {
            status: CalibrationStatus::Completed {
                position_rms_m: 0.01,
                sample_count: 100,
            },
            has_calibration: true,
            ..save_failed
        },
    );
    assert_ne!(completed, previous);
    assert!(
        initial
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    assert!(
        thumbnail
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] == [244, 248, 252])
    );
}
