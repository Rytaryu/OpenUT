//! Delivery of watcher notices outside the engine.

mod console;

use std::io;

use openut_core::notice::Notice;

pub use console::Console;

pub trait NoticeSink {
    fn push(&mut self, notice: &Notice) -> io::Result<()>;
}
