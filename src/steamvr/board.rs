//! Calibration dashboard on the app's existing SteamVR client connection.

use std::fmt;
use std::sync::mpsc::{Receiver, TryRecvError};

use fontdue::Font;

use crate::calib_flow::{CalibrationStatus, CalibrationView};

use super::client::{
    Client, ClientState, MouseEvent, MouseEventKind, OverlayCommand, OverlayHandles, OverlayResult,
};

mod pixels;

#[cfg(test)]
mod tests;

const KEY: &str = "openut.calibration";
const WIDTH_M: f32 = 1.15;
const LEFT_BUTTON: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoardAction {
    Start,
    Cancel,
}

impl BoardAction {
    fn for_view(view: CalibrationView) -> Self {
        if matches!(view.status, CalibrationStatus::Collecting { .. }) {
            Self::Cancel
        } else {
            Self::Start
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BoardError {
    FontUnavailable,
    Overlay(String),
}

impl fmt::Display for BoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FontUnavailable => f.write_str("SteamVR の板に使う日本語書体が見つかりません。"),
            Self::Overlay(message) => write!(f, "SteamVR の板を表示できません。{message}"),
        }
    }
}

impl std::error::Error for BoardError {}

pub struct Dashboard {
    font: Font,
    overlay: BoardOverlay,
}

impl Dashboard {
    pub fn new() -> Result<Self, BoardError> {
        let font = pixels::system_font().ok_or(BoardError::FontUnavailable)?;
        Ok(Self {
            font,
            overlay: BoardOverlay::default(),
        })
    }

    /// Polls for the previous command's reply and queues at most one new command.
    pub fn update(
        &mut self,
        client: &Client,
        state: ClientState,
        view: &CalibrationView,
    ) -> Result<(), BoardError> {
        self.overlay.update(
            state,
            *view,
            |epoch, command| client.overlay_for_epoch(epoch, command),
            |view| match view {
                Some(view) => pixels::board(&self.font, view),
                None => pixels::thumbnail(&self.font),
            },
        )
    }

    /// Only the primary press on the app's current dashboard can act.
    pub fn press(&self, state: ClientState, event: MouseEvent) -> Option<BoardAction> {
        self.overlay.press(state, event)
    }

    pub fn shutdown(&mut self, client: &Client) {
        if let (Some(epoch), Some(handles)) = (self.overlay.epoch, self.overlay.handles) {
            let _ = client.overlay_for_epoch(
                epoch,
                OverlayCommand::Destroy {
                    handle: handles.dashboard,
                },
            );
        }
        self.overlay = BoardOverlay::default();
        // The client also owns any dashboard still being created or destroyed.
    }
}

#[derive(Default)]
struct BoardOverlay {
    epoch: Option<u64>,
    handles: Option<OverlayHandles>,
    stage: Stage,
    pending: Option<Receiver<OverlayResult>>,
    shown: Option<CalibrationView>,
}

#[derive(Default, Clone, Copy)]
enum Stage {
    #[default]
    Create,
    Width,
    Mouse,
    Thumbnail,
    Ready,
    Paint(CalibrationView),
    Unavailable,
}

impl BoardOverlay {
    fn update(
        &mut self,
        state: ClientState,
        view: CalibrationView,
        mut submit: impl FnMut(u64, OverlayCommand) -> Result<Receiver<OverlayResult>, OverlayCommand>,
        draw: impl FnOnce(Option<CalibrationView>) -> Vec<u8>,
    ) -> Result<(), BoardError> {
        let epoch = state.connected.then_some(state.connection_epoch);
        if self.epoch != epoch {
            *self = Self {
                epoch,
                ..Self::default()
            };
        }
        let Some(epoch) = epoch else {
            return Ok(());
        };
        if let Some(reply) = self.pending.as_ref() {
            let result = match reply.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    Err("SteamVR から応答を受け取れませんでした。".into())
                }
            };
            self.pending = None;
            if let Err(error) = self.acknowledge(result) {
                self.stage = Stage::Unavailable;
                self.remove_overlay(epoch, &mut submit);
                return Err(error);
            }
        }
        if matches!(self.stage, Stage::Unavailable) {
            self.remove_overlay(epoch, &mut submit);
            return Ok(());
        }
        if matches!(self.stage, Stage::Ready | Stage::Paint(_)) {
            self.stage = if self.shown == Some(view) {
                Stage::Ready
            } else {
                Stage::Paint(view)
            };
        }
        let command = match (self.stage, self.handles) {
            (Stage::Create, _) => OverlayCommand::CreateDashboard {
                key: KEY.into(),
                name: "OpenUT".into(),
            },
            (Stage::Width, Some(handles)) => OverlayCommand::SetWidth {
                handle: handles.dashboard,
                meters: WIDTH_M,
            },
            (Stage::Mouse, Some(handles)) => OverlayCommand::SetMouseScale {
                handle: handles.dashboard,
                width: pixels::WIDTH as f32,
                height: pixels::HEIGHT as f32,
            },
            (Stage::Thumbnail, Some(handles)) => OverlayCommand::SetImage {
                handle: handles.thumbnail,
                width: pixels::THUMBNAIL,
                height: pixels::THUMBNAIL,
                rgba: draw(None),
            },
            (Stage::Paint(rendered), Some(handles)) => OverlayCommand::SetImage {
                handle: handles.dashboard,
                width: pixels::WIDTH,
                height: pixels::HEIGHT,
                rgba: draw(Some(rendered)),
            },
            _ => return Ok(()),
        };
        self.pending = submit(epoch, command).ok();
        Ok(())
    }

    fn acknowledge(&mut self, result: OverlayResult) -> Result<(), BoardError> {
        let response = result.map_err(BoardError::Overlay)?;
        self.stage = match self.stage {
            Stage::Create => {
                self.handles = Some(response.ok_or_else(|| {
                    BoardError::Overlay("板のハンドルを受け取れませんでした。".into())
                })?);
                Stage::Width
            }
            Stage::Width => Stage::Mouse,
            Stage::Mouse => Stage::Thumbnail,
            Stage::Thumbnail => Stage::Ready,
            Stage::Paint(view) => {
                self.shown = Some(view);
                Stage::Ready
            }
            stage => stage,
        };
        Ok(())
    }

    fn remove_overlay(
        &mut self,
        epoch: u64,
        submit: &mut impl FnMut(u64, OverlayCommand) -> Result<Receiver<OverlayResult>, OverlayCommand>,
    ) {
        self.shown = None;
        if let Some(handles) = self.handles
            && submit(
                epoch,
                OverlayCommand::Destroy {
                    handle: handles.dashboard,
                },
            )
            .is_ok()
        {
            self.handles = None;
        }
    }

    fn press(&self, state: ClientState, event: MouseEvent) -> Option<BoardAction> {
        let owns_handle = self
            .handles
            .is_some_and(|handles| handles.dashboard == event.handle);
        if state.connected
            && self.epoch == Some(state.connection_epoch)
            && owns_handle
            && event.kind == MouseEventKind::ButtonDown
            && event.button == LEFT_BUTTON
            && pixels::button_contains(event.x, pixels::HEIGHT as f32 - event.y)
        {
            self.shown.map(BoardAction::for_view)
        } else {
            None
        }
    }
}
