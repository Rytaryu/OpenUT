//! Refresh only the process metadata needed by the server guard.

use std::io;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::guard::ServerGuard;

pub struct ProcessList {
    system: System,
}

impl ProcessList {
    pub fn new() -> io::Result<Self> {
        let system =
            System::new().map_err(|_| io::Error::other("プロセス一覧を初期化できません"))?;
        Ok(Self { system })
    }

    pub fn server_running(&mut self, guard: &ServerGuard) -> bool {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        guard.server_running(
            self.system
                .processes()
                .values()
                .map(|process| process.name()),
        )
    }
}
