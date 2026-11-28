//! Rasterize the provisional board with fonts installed on the local system.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::{Font, FontSettings};
use openut_space::calib::CalibrationError;

use crate::calib_flow::{CalibrationFailure, CalibrationStatus, CalibrationView, RotationProgress};

use super::BoardAction;

pub(super) const WIDTH: u32 = 736;
pub(super) const HEIGHT: u32 = 384;
pub(super) const THUMBNAIL: u32 = 128;

const BACKGROUND: [u8; 3] = [19, 26, 39];
const PANEL: [u8; 3] = [31, 44, 64];
const TEXT: [u8; 3] = [244, 248, 252];
const SECONDARY: [u8; 3] = [187, 204, 223];
const HIGHLIGHT: [u8; 3] = [117, 231, 210];
const INK: [u8; 3] = [14, 35, 43];

#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

const BUTTON: Rect = Rect {
    x: 32,
    y: 304,
    width: 288,
    height: 55,
};

pub(super) fn button_contains(x: f32, y: f32) -> bool {
    (BUTTON.x as f32..(BUTTON.x + BUTTON.width) as f32).contains(&x)
        && (BUTTON.y as f32..(BUTTON.y + BUTTON.height) as f32).contains(&y)
}

pub(super) fn system_font() -> Option<Font> {
    let windows = env::var_os("WINDIR")
        .filter(|path| !path.is_empty())
        .or_else(|| env::var_os("SystemRoot").filter(|path| !path.is_empty()))?;
    let path = PathBuf::from(windows).join("Fonts");
    font_in(&path)
}

pub(super) fn font_in(path: &Path) -> Option<Font> {
    // The medium Yu Gothic collection includes the regular UI face. Prefer its
    // reported name so the ordering does not depend on the collection version.
    for (file, family) in [
        ("yugothm.ttc", "Yu Gothic UI"),
        ("meiryo.ttc", "Meiryo"),
        ("msgothic.ttc", "MS Gothic"),
    ] {
        if let Ok(bytes) = fs::read(path.join(file))
            && let Some(font) = load_font(&bytes, family)
        {
            return Some(font);
        }
    }
    None
}

pub(super) fn load_font(bytes: &[u8], family: &str) -> Option<Font> {
    let count = if let [b't', b't', b'c', b'f', _, _, _, _, a, b, c, d, ..] = bytes {
        u32::from_be_bytes([*a, *b, *c, *d]).min(32)
    } else {
        1
    };
    let mut fallback = None;
    for collection_index in 0..count {
        let settings = FontSettings {
            collection_index,
            load_substitutions: false,
            ..FontSettings::default()
        };
        let Ok(font) = Font::from_bytes(bytes, settings) else {
            continue;
        };
        if !"キャリブレーション動位置回設定重ね手首済終"
            .chars()
            .all(|character| font.has_glyph(character))
        {
            continue;
        }
        if font.name().is_some_and(|name| {
            name.strip_suffix(" Regular")
                .unwrap_or(name)
                .eq_ignore_ascii_case(family)
        }) {
            return Some(font);
        }
        fallback.get_or_insert(font);
    }
    fallback
}

pub(super) fn board(font: &Font, view: CalibrationView) -> Vec<u8> {
    let mut canvas = Canvas::new(WIDTH, HEIGHT);
    canvas.text(
        font,
        "OpenUT  /  キャリブレーション",
        32.0,
        22.0,
        23.0,
        SECONDARY,
    );

    let (heading, detail) = status_text(view.status);
    canvas.text(font, heading, 32.0, 75.0, 33.0, TEXT);
    canvas.text(font, detail, 32.0, 136.0, 24.0, SECONDARY);

    if let CalibrationStatus::Collecting { motion } = view.status {
        let (progress, message) = match motion {
            RotationProgress::None => (0, "手首を回し始めてください"),
            RotationProgress::OneAxis => (1, "もう一つの向きにも回してください"),
            RotationProgress::Ready => (2, "必要な動きが集まりました"),
        };
        canvas.text(font, message, 32.0, 224.0, 20.0, TEXT);
        for index in 0..2 {
            let color = if index < progress { HIGHLIGHT } else { PANEL };
            canvas.fill(
                Rect {
                    x: 32 + index * 106,
                    y: 271,
                    width: 98,
                    height: 6,
                },
                color,
            );
        }
    } else if view.needs_recalibration && matches!(view.status, CalibrationStatus::Failed(_)) {
        canvas.text(
            font,
            "もう一度キャリブレーションしてください。",
            32.0,
            232.0,
            20.0,
            TEXT,
        );
    }

    let action = BoardAction::for_view(view);
    let (label, fill, text) = match action {
        BoardAction::Start => ("始める", HIGHLIGHT, INK),
        BoardAction::Cancel => ("やめる", PANEL, TEXT),
    };
    canvas.fill(BUTTON, fill);
    canvas.text(
        font,
        label,
        (BUTTON.x + 20) as f32,
        (BUTTON.y + 9) as f32,
        24.0,
        text,
    );
    canvas.pixels
}

pub(super) fn thumbnail(font: &Font) -> Vec<u8> {
    let mut canvas = Canvas::new(THUMBNAIL, THUMBNAIL);
    canvas.fill(
        Rect {
            x: 19,
            y: 28,
            width: 90,
            height: 5,
        },
        HIGHLIGHT,
    );
    canvas.text(font, "OpenUT", 18.0, 54.0, 23.0, TEXT);
    canvas.pixels
}

fn status_text(status: CalibrationStatus) -> (&'static str, &'static str) {
    match status {
        CalibrationStatus::Needed => (
            "キャリブレーションが必要です",
            "トラッカーとコントローラーを用意してください。",
        ),
        CalibrationStatus::Calibrated => (
            "キャリブレーション済み",
            "室内の位置がずれたときは、もう一度行ってください。",
        ),
        CalibrationStatus::Collecting { .. } => (
            "トラッカーとコントローラーを動かす",
            "片手で重ねて持ち、ゆっくり 8 の字を描きながら\n手首をいろいろな向きに回してください。",
        ),
        CalibrationStatus::Completed { .. } => (
            "キャリブレーションが終わりました",
            "ダッシュボードを閉じて、トラッカーの位置を確かめてください。",
        ),
        CalibrationStatus::Failed(reason) => {
            ("キャリブレーションに失敗しました", failure_text(reason))
        }
        CalibrationStatus::NeedsRecalibration => (
            "キャリブレーションをやり直してください",
            "SteamVR の位置が変わりました。",
        ),
    }
}

fn failure_text(reason: CalibrationFailure) -> &'static str {
    match reason {
        CalibrationFailure::Fit(CalibrationError::MoreRotation) => {
            "手首をいろいろな向きに回してください。"
        }
        CalibrationFailure::Fit(CalibrationError::Uncorrelated) => {
            "トラッカーとコントローラーを重ねたまま動かしてください。"
        }
        CalibrationFailure::Fit(CalibrationError::GripSlipped) => {
            "トラッカーとコントローラーがずれないように持ってください。"
        }
        CalibrationFailure::Fit(CalibrationError::AxisMismatch) => {
            "向きが合いませんでした。もう一度やり直してください。"
        }
        CalibrationFailure::Fit(CalibrationError::ScaleOutOfRange) => {
            "トラッカーとコントローラーの動きの大きさが合いませんでした。"
        }
        CalibrationFailure::Fit(CalibrationError::TimedOut) => {
            "時間内に十分な動きが集まりませんでした。"
        }
        CalibrationFailure::SpaceUnavailable => "SteamVR の位置を確認できませんでした。",
        CalibrationFailure::SpaceChanged => "途中で SteamVR の位置が変わりました。",
    }
}

struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Self {
        let width = width as usize;
        let height = height as usize;
        let [r, g, b] = BACKGROUND;
        Self {
            width,
            height,
            pixels: [r, g, b, 255].repeat(width * height),
        }
    }

    fn fill(&mut self, rect: Rect, color: [u8; 3]) {
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                self.blend(x as usize, y as usize, color, 255);
            }
        }
    }

    fn text(&mut self, font: &Font, text: &str, x: f32, y: f32, size: f32, color: [u8; 3]) {
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x,
            y,
            max_width: Some(self.width as f32 - x - 28.0),
            ..LayoutSettings::default()
        });
        layout.append(&[font], &TextStyle::new(text, size, 0));
        for glyph in layout.glyphs() {
            let (metrics, coverage) = font.rasterize_config(glyph.key);
            if metrics.width == 0 {
                continue;
            }
            for (index, alpha) in coverage.into_iter().enumerate() {
                let x = glyph.x as i32 + (index % metrics.width) as i32;
                let y = glyph.y as i32 + (index / metrics.width) as i32;
                if let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) {
                    self.blend(x, y, color, alpha);
                }
            }
        }
    }

    fn blend(&mut self, x: usize, y: usize, color: [u8; 3], alpha: u8) {
        if x >= self.width || y >= self.height || alpha == 0 {
            return;
        }
        let index = (y * self.width + x) * 4;
        let alpha = u16::from(alpha);
        for (output, input) in self.pixels[index..index + 3].iter_mut().zip(color) {
            *output = ((u16::from(input) * alpha + u16::from(*output) * (255 - alpha)) / 255) as u8;
        }
    }
}
