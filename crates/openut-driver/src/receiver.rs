//! Local pose input. This worker never calls the host.

use std::collections::VecDeque;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use openut_bridge::{MAX_PACKET_LEN, Packet, decode};

pub const DEFAULT_PORT: u16 = 27_316;
pub const PACKET_CAPACITY: usize = 1_024;
pub const LOG_CAPACITY: usize = 64;
const SOCKET_WAIT: Duration = Duration::from_millis(100);
const BIND_RETRY: Duration = Duration::from_secs(10);
const MAX_DATAGRAM_LEN: usize = 65_536;

#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedPacket {
    pub packet: Packet,
    pub received_at: Instant,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReceiverStats {
    pub dropped_packets: u64,
    pub dropped_logs: u64,
    pub invalid_packets: u64,
    pub foreign_packets: u64,
    pub bind_failures: u64,
    pub receive_failures: u64,
}

#[derive(Default)]
struct Counters {
    dropped_packets: AtomicU64,
    dropped_logs: AtomicU64,
    invalid_packets: AtomicU64,
    foreign_packets: AtomicU64,
    bind_failures: AtomicU64,
    receive_failures: AtomicU64,
}

#[derive(Default)]
pub struct Inbox {
    packets: Mutex<VecDeque<ReceivedPacket>>,
    logs: Mutex<VecDeque<String>>,
    counters: Counters,
}

impl Inbox {
    pub fn take_packets(&self) -> VecDeque<ReceivedPacket> {
        take(&self.packets)
    }

    pub fn take_logs(&self) -> VecDeque<String> {
        take(&self.logs)
    }

    pub fn push_packet(&self, packet: ReceivedPacket) {
        push_bounded(
            &self.packets,
            packet,
            PACKET_CAPACITY,
            &self.counters.dropped_packets,
        );
    }

    pub fn push_log(&self, line: String) {
        push_bounded(&self.logs, line, LOG_CAPACITY, &self.counters.dropped_logs);
    }

    pub fn stats(&self) -> ReceiverStats {
        ReceiverStats {
            dropped_packets: self.counters.dropped_packets.load(Ordering::Relaxed),
            dropped_logs: self.counters.dropped_logs.load(Ordering::Relaxed),
            invalid_packets: self.counters.invalid_packets.load(Ordering::Relaxed),
            foreign_packets: self.counters.foreign_packets.load(Ordering::Relaxed),
            bind_failures: self.counters.bind_failures.load(Ordering::Relaxed),
            receive_failures: self.counters.receive_failures.load(Ordering::Relaxed),
        }
    }

    fn accept(&self, from: SocketAddr, data: &[u8], received_at: Instant) {
        if from.ip() != Ipv4Addr::LOCALHOST {
            self.counters
                .foreign_packets
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        if data.len() <= MAX_PACKET_LEN
            && let Ok(packet) = decode(data)
        {
            self.push_packet(ReceivedPacket {
                packet,
                received_at,
            });
        } else {
            self.counters
                .invalid_packets
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn take<T>(queue: &Mutex<VecDeque<T>>) -> VecDeque<T> {
    std::mem::take(&mut *queue.lock().unwrap_or_else(PoisonError::into_inner))
}

fn push_bounded<T>(queue: &Mutex<VecDeque<T>>, item: T, limit: usize, dropped: &AtomicU64) {
    let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
    if queue.len() == limit {
        queue.pop_front();
        // The frame that drains this queue must be able to observe the drop.
        dropped.fetch_add(1, Ordering::Relaxed);
    }
    queue.push_back(item);
}

pub struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(port: u16, inbox: Arc<Inbox>) -> io::Result<Self> {
        Self::spawn_task(move |stop| {
            if catch_unwind(AssertUnwindSafe(|| run(port, stop, &inbox))).is_err() {
                inbox.push_log(String::from("Local pose receiver stopped unexpectedly."));
            }
        })
    }

    fn spawn_task(task: impl FnOnce(&AtomicBool) + Send + 'static) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name(String::from("openut-pose-receiver"))
            .spawn(move || task(&thread_stop))?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    pub fn stop_and_join(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.stop.store(true, Ordering::Release);
            handle.thread().unpark();
            let _ = handle.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.finish();
    }
}

fn run(port: u16, stop: &AtomicBool, inbox: &Inbox) {
    while !stop.load(Ordering::Acquire) {
        match bind(port) {
            Ok(socket) => {
                if let Err(error) = receive(&socket, stop, inbox) {
                    inbox
                        .counters
                        .receive_failures
                        .fetch_add(1, Ordering::Relaxed);
                    inbox.push_log(format!(
                        "Local pose socket read failed ({:?}). Retrying.",
                        error.kind()
                    ));
                }
            }
            Err(error) => {
                inbox.counters.bind_failures.fetch_add(1, Ordering::Relaxed);
                inbox.push_log(format!(
                    "Local pose socket bind failed ({:?}). Retrying.",
                    error.kind()
                ));
            }
        }
        wait_to_retry(stop);
    }
}

fn bind(port: u16) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, port))?;
    socket.set_read_timeout(Some(SOCKET_WAIT))?;
    Ok(socket)
}

fn receive(socket: &UdpSocket, stop: &AtomicBool, inbox: &Inbox) -> io::Result<()> {
    // Receive even oversized datagrams intact. An undersized socket buffer can
    // report a socket error on one host and a truncated read on another.
    let mut data = [0; MAX_DATAGRAM_LEN];
    while !stop.load(Ordering::Acquire) {
        match socket.recv_from(&mut data) {
            Ok((len, from)) => inbox.accept(from, &data[..len], Instant::now()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn wait_to_retry(stop: &AtomicBool) {
    let start = Instant::now();
    while !stop.load(Ordering::Acquire) {
        let remaining = BIND_RETRY.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            break;
        }
        thread::park_timeout(remaining.min(SOCKET_WAIT));
    }
}

#[cfg(test)]
mod tests;
