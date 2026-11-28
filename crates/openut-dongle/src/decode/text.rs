use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceInfoField {
    Nads,
    Nass,
    Nasi,
    Napi,
    Nav1,
    Nari,
    Nagn,
    Nana,
    Nadn,
    Nadi,
}

/// An opaque value that must not be emitted through ordinary formatting.
#[derive(Clone, Eq, PartialEq)]
pub struct Redactable(String);

impl Redactable {
    /// Access the original for controlled processing, such as anonymization.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Redactable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Redactable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrackerMessage {
    DeviceInfo {
        field: DeviceInfoField,
        value: Redactable,
    },
    /// Its meaning is ambiguous between the two protocol sources.
    Nazz,
    MapStatus {
        a: i32,
        b: i32,
    },
    LambdaStatus(Vec<i32>),
    SlamAttributes(Redactable),
    SlamStatus {
        key: u8,
        value: i64,
    },
    SlamError {
        code: u32,
    },
    DeviceError(Redactable),
    Calibration(Redactable),
    WifiHostInfo,
    WifiInfoRequest,
    WifiConnected {
        ok: bool,
    },
    WifiSetting(Redactable),
    PowerOff,
    Reset,
    Unknown {
        prefix: String,
    },
}

pub(super) fn decode(bytes: &[u8]) -> Result<TrackerMessage, &'static str> {
    use TrackerMessage::*;

    if !bytes.iter().all(|byte| (b' '..=b'~').contains(byte)) {
        return Err("tracker text is not printable ASCII");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "invalid tracker text")?;
    let text = if bytes.first().is_some_and(u8::is_ascii_digit) {
        &text[1..]
    } else {
        text
    };
    if text.is_empty() {
        return Err("tracker text is empty");
    }

    let marker = match text {
        "NAZZ" => Some(Nazz),
        "WS" => Some(WifiInfoRequest),
        "APF" => Some(PowerOff),
        "APR" => Some(Reset),
        _ => None,
    };
    if let Some(message) = marker {
        return Ok(message);
    }

    if let Some((field, value)) = device_info(text) {
        return Ok(DeviceInfo {
            field,
            value: Redactable(value.to_owned()),
        });
    }

    if let Some(value) = text.strip_prefix("MS") {
        let (a, b) = number_pair(value)?;
        return Ok(MapStatus { a, b });
    }
    if let Some(value) = text.strip_prefix("LS") {
        return value
            .split(',')
            .map(number)
            .collect::<Result<_, _>>()
            .map(LambdaStatus);
    }
    if let Some(value) = text.strip_prefix("P61:") {
        let (key, value) = number_pair(value)?;
        return Ok(SlamStatus { key, value });
    }
    if let Some(value) = text.strip_prefix("P82:0:") {
        return Ok(SlamError {
            code: number(value)?,
        });
    }
    if let Some(value) = text.strip_prefix("WC") {
        return Ok(WifiConnected {
            ok: number::<i32>(value)? > 0,
        });
    }

    if text.starts_with("WH") {
        return Ok(WifiHostInfo);
    }
    if let Some(value) = text.strip_prefix("LP") {
        return Ok(SlamAttributes(Redactable(value.to_owned())));
    }
    if let Some(value) = text.strip_prefix("DEC") {
        return Ok(DeviceError(Redactable(value.to_owned())));
    }
    if text.starts_with(['C', 'c']) {
        return Ok(Calibration(Redactable(text.to_owned())));
    }
    if ["Ws", "Wp", "Wt"]
        .into_iter()
        .any(|prefix| text.starts_with(prefix))
    {
        return Ok(WifiSetting(Redactable(text.to_owned())));
    }

    Ok(Unknown {
        prefix: text[..text.len().min(2)].to_owned(),
    })
}

fn device_info(text: &str) -> Option<(DeviceInfoField, &str)> {
    use DeviceInfoField::*;

    let (kind, value) = text.split_at_checked(4)?;
    let field = match kind {
        "NADS" => Nads,
        "NASS" => Nass,
        "NASI" => Nasi,
        "NAPI" => Napi,
        "NAV1" => Nav1,
        "NARI" => Nari,
        "NAGN" => Nagn,
        "NANA" => Nana,
        "NADN" => Nadn,
        "NADI" => Nadi,
        _ => return None,
    };
    Some((field, value))
}

fn number<T: FromStr>(text: &str) -> Result<T, &'static str> {
    text.parse().map_err(|_| "invalid tracker text number")
}

fn number_pair<A: FromStr, B: FromStr>(text: &str) -> Result<(A, B), &'static str> {
    let (first, second) = text.split_once(',').ok_or("missing tracker text number")?;
    Ok((number(first)?, number(second)?))
}
