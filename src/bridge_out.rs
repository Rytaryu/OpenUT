//! Sends bridge datagrams without blocking the tracking loop.

use std::io;
use std::net::{Ipv4Addr, UdpSocket};

use openut_bridge::{Packet, encode};

/// A single nonblocking datagram attempt.
pub trait DatagramSink {
    fn try_send(&mut self, bytes: &[u8]) -> io::Result<usize>;
}

/// A UDP socket connected to the SteamVR driver on this PC.
pub struct LoopbackSender(UdpSocket);

impl DatagramSink for LoopbackSender {
    fn try_send(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.send(bytes)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SendStats {
    /// Datagrams accepted by the socket.
    pub sent: u64,
    /// Packets that could not be encoded or sent in a single attempt.
    pub dropped: u64,
}

pub struct BridgeOut<S = LoopbackSender> {
    sink: S,
    stats: SendStats,
}

impl BridgeOut {
    /// Binds an ephemeral source port and selects the local driver's port.
    pub fn bind(port: u16) -> io::Result<Self> {
        if port == 0 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        socket.set_nonblocking(true)?;
        socket.connect((Ipv4Addr::LOCALHOST, port))?;
        Ok(Self::with_sink(LoopbackSender(socket)))
    }
}

impl<S: DatagramSink> BridgeOut<S> {
    pub fn with_sink(sink: S) -> Self {
        Self {
            sink,
            stats: SendStats::default(),
        }
    }

    /// Tries once and drops the packet on an encoding or socket error.
    pub fn send(&mut self, packet: &Packet) -> bool {
        let sent = encode(packet).is_ok_and(|bytes| {
            self.sink
                .try_send(&bytes)
                .is_ok_and(|len| len == bytes.len())
        });
        let count = if sent {
            &mut self.stats.sent
        } else {
            &mut self.stats.dropped
        };
        *count = count.saturating_add(1);
        sent
    }

    pub fn stats(&self) -> SendStats {
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openut_bridge::{MAX_PACKET_LEN, Payload, decode};

    fn packet() -> Packet {
        Packet {
            session: 1,
            seq: 2,
            payload: Payload::DeviceList(Vec::new()),
        }
    }

    #[derive(Default)]
    struct BusyThenReady {
        attempts: usize,
    }

    impl DatagramSink for BusyThenReady {
        fn try_send(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.attempts += 1;
            match self.attempts {
                1 => Err(io::ErrorKind::WouldBlock.into()),
                2 => Ok(bytes.len()),
                3 => Err(io::ErrorKind::ConnectionRefused.into()),
                _ => Ok(bytes.len() - 1),
            }
        }
    }

    #[test]
    fn does_not_retry_or_queue_packets_on_a_busy_socket() {
        let mut output = BridgeOut::with_sink(BusyThenReady::default());
        let packet = packet();
        assert!(!output.send(&packet));
        assert_eq!(output.sink.attempts, 1);
        assert_eq!(
            output.stats(),
            SendStats {
                sent: 0,
                dropped: 1
            }
        );
        assert!(output.send(&packet));
        assert!(!output.send(&packet));
        assert!(!output.send(&packet));

        let invalid = Packet {
            session: 0,
            ..packet
        };
        assert!(!output.send(&invalid));
        assert_eq!(output.sink.attempts, 4);
        assert_eq!(
            output.stats(),
            SendStats {
                sent: 1,
                dropped: 4
            }
        );
    }

    #[test]
    fn uses_only_ipv4_loopback_and_a_distinct_ephemeral_source_port() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let destination = receiver.local_addr().unwrap();
        let mut output = BridgeOut::bind(destination.port()).unwrap();
        let source = output.sink.0.local_addr().unwrap();
        assert_eq!(source.ip(), Ipv4Addr::LOCALHOST);
        assert_ne!(source.port(), 0);
        assert_ne!(source.port(), destination.port());
        assert_eq!(output.sink.0.peer_addr().unwrap(), destination);

        let packet = packet();
        assert!(output.send(&packet));
        let mut bytes = [0; MAX_PACKET_LEN];
        let (length, actual_source) = receiver.recv_from(&mut bytes).unwrap();
        assert_eq!(actual_source, source);
        assert_eq!(decode(&bytes[..length]).unwrap(), packet);
    }
}
