//! Live system information (CPU, memory, disks, processes) for all platforms.

use sysinfo::{Disks, System};

#[derive(Debug, Clone, Default)]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub total: u64,
    pub available: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub os: String,
    pub kernel: String,
    pub host: String,
    pub uptime_secs: u64,
    pub cpu_brand: String,
    pub cpu_total: f32,
    pub cpu_cores: Vec<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub disks: Vec<DiskInfo>,
    pub top_procs: Vec<ProcInfo>,
    pub proc_count: usize,
}

impl Snapshot {
    pub fn mem_fraction(&self) -> f32 {
        if self.mem_total == 0 {
            0.0
        } else {
            self.mem_used as f32 / self.mem_total as f32
        }
    }

    pub fn uptime_text(&self) -> String {
        let s = self.uptime_secs;
        format!(
            "{}d {}h {}m",
            s / 86_400,
            (s % 86_400) / 3600,
            (s % 3600) / 60
        )
    }

    /// Plain-text summary, handy for giving Claude context.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "OS: {} (kernel {})\nHost: {}\nUptime: {}\nCPU: {} - {:.0}% across {} cores\nMemory: {} / {}\n",
            self.os,
            self.kernel,
            self.host,
            self.uptime_text(),
            self.cpu_brand,
            self.cpu_total,
            self.cpu_cores.len(),
            crate::human_bytes(self.mem_used),
            crate::human_bytes(self.mem_total),
        );
        for d in &self.disks {
            s.push_str(&format!(
                "Disk {} ({}): {} free of {}\n",
                d.mount,
                d.name,
                crate::human_bytes(d.available),
                crate::human_bytes(d.total)
            ));
        }
        s
    }
}

pub struct Monitor {
    sys: System,
    disks: Disks,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self {
            sys,
            disks: Disks::new_with_refreshed_list(),
        }
    }

    /// Refresh and return a snapshot. CPU figures need two refreshes a
    /// short time apart to be meaningful, so call this periodically.
    pub fn snapshot(&mut self, top_n: usize) -> Snapshot {
        self.sys.refresh_cpu();
        self.sys.refresh_memory();
        self.sys.refresh_processes();
        self.disks.refresh();

        let mut procs: Vec<ProcInfo> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| ProcInfo {
                pid: pid.as_u32(),
                name: p.name().to_string(),
                cpu: p.cpu_usage(),
                memory: p.memory(),
            })
            .collect();
        let proc_count = procs.len();
        procs.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.memory.cmp(&a.memory))
        });
        procs.truncate(top_n);

        Snapshot {
            os: System::long_os_version().unwrap_or_else(|| crate::host_os().to_string()),
            kernel: System::kernel_version().unwrap_or_default(),
            host: System::host_name().unwrap_or_default(),
            uptime_secs: System::uptime(),
            cpu_brand: self
                .sys
                .cpus()
                .first()
                .map(|c| c.brand().trim().to_string())
                .unwrap_or_default(),
            cpu_total: self.sys.global_cpu_info().cpu_usage(),
            cpu_cores: self.sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            disks: self
                .disks
                .list()
                .iter()
                .map(|d| DiskInfo {
                    name: d.name().to_string_lossy().into_owned(),
                    mount: d.mount_point().display().to_string(),
                    total: d.total_space(),
                    available: d.available_space(),
                })
                .collect(),
            top_procs: procs,
            proc_count,
        }
    }

    /// Ask the OS to end a process.
    pub fn kill(&mut self, pid: u32) -> bool {
        self.sys
            .process(sysinfo::Pid::from_u32(pid))
            .map(|p| p.kill())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_has_memory() {
        let mut m = Monitor::new();
        let s = m.snapshot(5);
        assert!(s.mem_total > 0);
        assert!(!s.cpu_cores.is_empty());
        assert!(s.top_procs.len() <= 5);
        assert!(s.summary().contains("Memory"));
    }
}
