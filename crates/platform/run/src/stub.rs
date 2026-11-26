use crate::{CommandLine, ProcessSink};

pub struct Process {
    never: std::convert::Infallible,
}

impl Process {
    pub fn spawn(_line: &CommandLine, _sink: ProcessSink) -> Result<Process, String> {
        Err("this build cannot run programs".to_owned())
    }

    pub fn stop(&self) {
        match self.never {}
    }

    pub fn kill(&self) {
        match self.never {}
    }

    pub fn pid(&self) -> Option<u32> {
        match self.never {}
    }

    pub fn has_exited(&self) -> bool {
        match self.never {}
    }
}

#[derive(Clone, Default)]
pub struct Programs;

impl Programs {
    pub fn spawn(&self, _command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "this build cannot run programs"))
    }

    pub fn stopping(&self) -> bool {
        false
    }

    pub fn add(&self, _pid: u32) {}

    pub fn remove(&self, _pid: u32) {}
}

impl zen_project::inspect::Programs for Programs {
    fn interrupt(&self) {}

    fn kill(&self) {}

    fn running(&self) -> bool {
        false
    }
}

pub fn own_group(_command: &mut std::process::Command) {}

pub fn kill_group(_pid: u32) {}
