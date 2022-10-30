// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use anyhow::{Context, Error, Result};
use nix::sys::signal::{kill, SIGHUP};
use sysinfo::{
    self, get_current_pid, Pid, PidExt, Process, ProcessExt, ProcessRefreshKind, RefreshKind,
    System, SystemExt,
};

/// Sends SIGHUP to all process with a different pid and the same name
pub fn trigger_reload() -> Result<()> {
    let target_processes = find_target_processes()?;
    if target_processes.is_empty() {
        println!("There is currently no other controller process running");
        Ok(())
    } else {
        send_sighup_to_proccesses(target_processes)
    }
}

/// Looks for a proccess with the same name and a different pid, returns [`Option<Pid>`]
fn find_target_processes() -> Result<Vec<Pid>> {
    let system = System::new_with_specifics(
        RefreshKind::default().with_processes(ProcessRefreshKind::everything()),
    );
    let current_process = get_current_process(&system)?;
    Ok(search_for_target_processes(&system, current_process))
}

/// Send SIGHUP to each process in [`Vec<Pid>`]
fn send_sighup_to_proccesses(processes: Vec<Pid>) -> Result<()> {
    processes
        .into_iter()
        .try_for_each(|pid| kill(nix::unistd::Pid::from_raw(pid.as_u32() as i32), SIGHUP))
        .map_err(Error::from)
}

/// Returns the [`Process`] of the current running application
fn get_current_process(system: &System) -> Result<&Process> {
    let pid = get_current_pid().map_err(Error::msg)?;
    system
        .process(pid)
        .context("Failed to get current process, that was not to expect")
}

/// Iterates over all processes to find processes with same name as the current process and a different pid
fn search_for_target_processes(system: &System, current_process: &Process) -> Vec<Pid> {
    system
        .processes()
        .iter()
        .filter(|(pid, process)| {
            **pid != current_process.pid() && process.name() == current_process.name()
        })
        .map(|(pid, _process)| *pid)
        .collect()
}
