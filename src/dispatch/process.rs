use nix::{
    errno::Errno,
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use std::time::Duration;
use tokio::{process::Child, time::Instant};

/// The group outlives its leader: even normal shell exit must settle descendants.
pub(super) struct ProcessGroup {
    pid: Pid,
    armed: bool,
}

impl ProcessGroup {
    pub fn new(pid: u32) -> Self {
        Self {
            pid: Pid::from_raw(pid as i32),
            armed: true,
        }
    }

    fn exists(&self) -> bool {
        !matches!(killpg(self.pid, None), Err(Errno::ESRCH))
    }

    pub async fn settle(&mut self, child: &mut Child) {
        let _ = killpg(self.pid, Signal::SIGTERM);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let _ = child.try_wait();
            if !self.exists() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = killpg(self.pid, Signal::SIGKILL);
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let _ = child.wait().await;
        self.armed = false;
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if self.armed {
            let _ = killpg(self.pid, Signal::SIGKILL);
        }
    }
}
