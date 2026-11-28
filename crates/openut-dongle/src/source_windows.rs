//! Windows HID input adapter. Keep device handles and identifiers private.

use std::fmt;
use std::time::Duration;

use hidapi::{DeviceInfo, HidApi, HidDevice, HidError};

use crate::record::Device;
use crate::run::{ReadOutcome, ReportSource};

const HTC_VENDOR: u16 = 0x0bb4;
const ULTIMATE_DONGLE: u16 = 0x0350;
// Windows reports the maximum input length in a u16. Keep unexpected report
// sizes intact so the first device trial can see what the backend returned.
const INPUT_CAPACITY: usize = u16::MAX as usize;

pub struct DeviceSummary {
    pub device: Device,
    pub product: Option<String>,
}

pub struct DeviceList {
    api: HidApi,
}

impl DeviceList {
    pub fn new() -> Result<Self, OpenError> {
        let api = HidApi::new().map_err(OpenError::backend)?;
        Ok(Self { api })
    }

    pub fn devices(&self) -> impl Iterator<Item = DeviceSummary> + '_ {
        self.api
            .device_list()
            .filter(|info| info.vendor_id() == HTC_VENDOR)
            .map(|info| DeviceSummary {
                device: device(info),
                product: info.product_string().and_then(display_product),
            })
    }

    pub fn dongle(&self) -> Result<Device, OpenError> {
        self.dongle_entry().map(device)
    }

    pub fn open_dongle(&self) -> Result<HidReportSource, OpenError> {
        let info = self.dongle_entry()?;
        let handle = self
            .api
            .open_path(info.path())
            .map_err(OpenError::backend)?;
        Ok(HidReportSource {
            handle,
            buffer: vec![0; INPUT_CAPACITY].into_boxed_slice(),
        })
    }

    fn dongle_entry(&self) -> Result<&DeviceInfo, OpenError> {
        let mut candidates = self.api.device_list().filter(|info| {
            info.vendor_id() == HTC_VENDOR
                && info.product_id() == ULTIMATE_DONGLE
                // The Windows backend can omit the interface number for a
                // non-composite device. Accept it only as a unique candidate.
                && matches!(info.interface_number(), -1 | 0)
        });
        let selected = candidates.next().ok_or(OpenError::Missing)?;
        if candidates.next().is_some() {
            return Err(OpenError::Ambiguous);
        }
        Ok(selected)
    }
}

fn device(info: &DeviceInfo) -> Device {
    Device {
        vid: info.vendor_id(),
        pid: info.product_id(),
        interface: info.interface_number(),
        usage_page: info.usage_page(),
        usage: info.usage(),
    }
}

fn display_product(product: &str) -> Option<String> {
    let product = product.trim();
    (!product.is_empty()
        && product.chars().count() <= 80
        && !product.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        }))
    .then(|| product.to_owned())
}

/// Only the input interface is exposed. Dropping the adapter closes the handle.
pub struct HidReportSource {
    handle: HidDevice,
    buffer: Box<[u8]>,
}

impl ReportSource for HidReportSource {
    fn read_timeout(&mut self, timeout: Duration) -> ReadOutcome {
        let timeout_ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        match self.handle.read_timeout(&mut self.buffer, timeout_ms) {
            Ok(0) => ReadOutcome::Idle,
            Ok(count) => match self.buffer.get(..count) {
                Some(bytes) => ReadOutcome::Report(bytes.to_vec()),
                None => ReadOutcome::ReadError(0),
            },
            Err(error) => match os_code(&error) {
                // ERROR_DEV_NOT_EXIST, ERROR_NO_SUCH_DEVICE,
                // ERROR_DEVICE_NOT_CONNECTED, ERROR_DEVICE_REMOVED.
                55 | 433 | 1167 | 1617 => ReadOutcome::Removed,
                code => ReadOutcome::ReadError(code),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenError {
    Missing,
    Ambiguous,
    Backend(i32),
}

impl OpenError {
    fn backend(error: HidError) -> Self {
        Self::Backend(os_code(&error))
    }

    /// Zero means the backend did not expose an operating-system error number.
    pub fn os_code(self) -> i32 {
        match self {
            Self::Backend(code) => code,
            Self::Missing | Self::Ambiguous => 0,
        }
    }
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("対象のドングルが見つかりません"),
            Self::Ambiguous => f.write_str("対象のドングルを 1 件に特定できません"),
            Self::Backend(0) => f.write_str("ドングルの一覧取得または接続に失敗しました"),
            Self::Backend(code) => write!(
                f,
                "ドングルの一覧取得または接続に失敗しました（OS 番号 {code}）"
            ),
        }
    }
}

impl std::error::Error for OpenError {}

fn os_code(error: &HidError) -> i32 {
    match error {
        HidError::IoError { error } => error.raw_os_error().unwrap_or(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::display_product;

    #[test]
    fn product_display_rejects_nonprinting_descriptors() {
        assert_eq!(
            display_product("  Synthetic HID product  ").as_deref(),
            Some("Synthetic HID product")
        );
        assert_eq!(display_product("name\x1b[2J"), None);
        assert_eq!(display_product("name\u{202e}suffix"), None);
        assert_eq!(display_product(&"x".repeat(81)), None);
    }
}
