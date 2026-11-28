//! Tracker identifiers used by the event source and state engine.

/// The event source's slot for a tracker during a connection.
/// A slot can be reused after disconnecting.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrackerKey(pub u16);

/// A tracker serial number, when the event source supplies one.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Serial(pub String);
