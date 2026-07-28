//! One-round-trip performance sampling.
//!
//! Each sample is one compound command over the existing session, parsed here
//! into numbers the pane can chart. Nothing is installed or privileged.
//!
//! CPU percentage is a delta between two `/proc/stat` readings, so the previous
//! one is kept on the session (`LiveSession::prev_cpu`) and the first sample
//! reports "no reading yet" rather than sleeping inside the command.
//!
//! macOS and BSD have no `/proc` and get a partial (load, uptime, disks) with a
//! note. Windows reports through CIM as JSON; its `LoadPercentage` is already
//! instantaneous and needs no delta.

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

use super::registry::LiveSession;
use super::OsFamily;
use crate::ssh::{SshError, SshResult};

/// Section separator for the compound commands. Improbable in real output.
const MARKER: &str = "---PAROLA---";

/// One sample, as the pane receives it. Fields are optional on purpose: a
/// platform reports what it can rather than faking zeros.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostMetrics {
    pub sampled_at_ms: i64,
    /// `None` until there are two readings to subtract, and on platforms
    /// with no counter to read.
    pub cpu_percent: Option<f64>,
    pub memory: Option<MemoryInfo>,
    /// 1, 5 and 15 minute load averages. `None` on Windows.
    pub load: Option<[f64; 3]>,
    pub uptime_seconds: Option<u64>,
    pub disks: Vec<DiskInfo>,
    /// Sentences explaining anything absent above.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total_kb: u64,
    pub available_kb: u64,
    pub used_percent: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub mount: String,
    pub total_kb: u64,
    pub used_kb: u64,
    pub used_percent: f64,
}

/// Cumulative CPU counters from one `/proc/stat` reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    pub busy: u64,
    pub total: u64,
}

/// The whole Linux sample in one exec.
const LINUX_COMMAND: &str = "cat /proc/stat; echo ---PAROLA---; cat /proc/meminfo; \
     echo ---PAROLA---; df -P -k; echo ---PAROLA---; cat /proc/uptime; \
     echo ---PAROLA---; cat /proc/loadavg";

/// What a /proc-less Unix can still answer.
const UNIX_FALLBACK_COMMAND: &str = "uptime; echo ---PAROLA---; df -P -k";

/// One PowerShell invocation, JSON out, so parsing does not depend on the
/// display locale. `LoadPercentage` is instantaneous - no delta needed.
const WINDOWS_COMMAND: &str = "powershell -NoProfile -NonInteractive -Command \
    \"@{ os = Get-CimInstance Win32_OperatingSystem | Select-Object \
    TotalVisibleMemorySize,FreePhysicalMemory,LastBootUpTime; \
    cpu = (Get-CimInstance Win32_Processor | Measure-Object -Property \
    LoadPercentage -Average).Average; \
    disks = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | \
    Select-Object DeviceID,Size,FreeSpace) } | ConvertTo-Json -Depth 3\"";

/// The command a sample runs on this OS. Pure.
pub fn sample_command(os: OsFamily) -> SshResult<&'static str> {
    match os {
        OsFamily::Linux => Ok(LINUX_COMMAND),
        OsFamily::Macos | OsFamily::Bsd => Ok(UNIX_FALLBACK_COMMAND),
        OsFamily::Windows => Ok(WINDOWS_COMMAND),
        OsFamily::Unknown => Err(SshError::unsupported(
            "The remote operating system is unknown, so no metrics command can be \
             chosen safely.",
        )),
    }
}

/// Take one sample from a live session.
pub async fn sample(live: &LiveSession) -> SshResult<HostMetrics> {
    let command = sample_command(live.os)?;
    let output = live.session.exec(command, None).await?;

    let sampled_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let metrics = match live.os {
        OsFamily::Linux => {
            // After the exec, so the lock is never held across an await.
            let previous = live.prev_cpu();
            let (metrics, current) = parse_linux(&output.stdout, previous, sampled_at_ms);
            if let Some(current) = current {
                live.set_prev_cpu(current);
            }
            metrics
        }
        OsFamily::Windows => parse_windows(&output.stdout, sampled_at_ms),
        _ => parse_unix_fallback(&output.stdout, sampled_at_ms),
    };

    Ok(metrics)
}

/// Parse the Linux compound output. Pure; returns the fresh CPU reading so
/// the caller can store it for the next delta.
pub fn parse_linux(
    stdout: &str,
    previous_cpu: Option<CpuTimes>,
    sampled_at_ms: i64,
) -> (HostMetrics, Option<CpuTimes>) {
    let mut sections = stdout.split(MARKER);
    let stat = sections.next().unwrap_or("");
    let meminfo = sections.next().unwrap_or("");
    let df = sections.next().unwrap_or("");
    let uptime = sections.next().unwrap_or("");
    let loadavg = sections.next().unwrap_or("");

    let current = parse_proc_stat(stat);
    let cpu_percent = match (previous_cpu, current) {
        (Some(previous), Some(current)) => cpu_percent(previous, current),
        _ => None,
    };

    let mut notes = Vec::new();
    if cpu_percent.is_none() {
        notes.push(
            "CPU usage needs two readings; it appears from the second sample on.".to_string(),
        );
    }

    (
        HostMetrics {
            sampled_at_ms,
            cpu_percent,
            memory: parse_meminfo(meminfo),
            load: parse_loadavg(loadavg),
            uptime_seconds: parse_proc_uptime(uptime),
            disks: parse_df(df),
            notes,
        },
        current,
    )
}

/// First line of `/proc/stat`: `cpu  user nice system idle iowait irq …`.
fn parse_proc_stat(text: &str) -> Option<CpuTimes> {
    let line = text.lines().find(|line| {
        line.starts_with("cpu ") || line.starts_with("cpu\t")
    })?;

    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|field| field.parse().ok())
        .collect();
    if fields.len() < 4 {
        return None;
    }

    // idle + iowait are idle; the rest of the first eight fields are busy.
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    let total: u64 = fields.iter().take(8).sum();

    Some(CpuTimes {
        busy: total.saturating_sub(idle),
        total,
    })
}

/// Percentage of non-idle time between two readings. Counters only grow, so a
/// shrink means a reboot - answered with "no reading", not a negative.
pub fn cpu_percent(previous: CpuTimes, current: CpuTimes) -> Option<f64> {
    if current.total <= previous.total || current.busy < previous.busy {
        return None;
    }
    let total = (current.total - previous.total) as f64;
    let busy = (current.busy - previous.busy) as f64;
    Some((busy / total * 100.0).clamp(0.0, 100.0))
}

/// `/proc/meminfo`: prefer `MemAvailable` (kernel ≥ 3.14); on older kernels
/// approximate it the way `free` used to, from free + buffers + cached.
fn parse_meminfo(text: &str) -> Option<MemoryInfo> {
    let field = |name: &str| -> Option<u64> {
        text.lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse().ok())
    };

    let total_kb = field("MemTotal:")?;
    let available_kb = field("MemAvailable:").or_else(|| {
        Some(field("MemFree:")? + field("Buffers:").unwrap_or(0) + field("Cached:").unwrap_or(0))
    })?;

    if total_kb == 0 {
        return None;
    }
    let used = total_kb.saturating_sub(available_kb) as f64;

    Some(MemoryInfo {
        total_kb,
        available_kb,
        used_percent: used / total_kb as f64 * 100.0,
    })
}

/// `df -P -k`: POSIX-format rows, sizes in KiB. Pseudo-filesystems that
/// mirror RAM say nothing about disks, so they are dropped.
fn parse_df(text: &str) -> Vec<DiskInfo> {
    text.lines()
        .skip(1) // header
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 6 {
                return None;
            }

            let filesystem = fields[0];
            if matches!(filesystem, "tmpfs" | "devtmpfs" | "efivarfs" | "overlay" | "none") {
                return None;
            }

            let total_kb: u64 = fields[1].parse().ok()?;
            let used_kb: u64 = fields[2].parse().ok()?;
            if total_kb == 0 {
                return None;
            }
            // Mount points may contain spaces: everything from field six on.
            let mount = fields[5..].join(" ");

            Some(DiskInfo {
                mount,
                total_kb,
                used_kb,
                used_percent: used_kb as f64 / total_kb as f64 * 100.0,
            })
        })
        .collect()
}

/// `/proc/uptime`: seconds-up, then idle time we do not need.
fn parse_proc_uptime(text: &str) -> Option<u64> {
    text.split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .map(|seconds| seconds as u64)
}

/// `/proc/loadavg`: three averages, then scheduler counts we do not need.
fn parse_loadavg(text: &str) -> Option<[f64; 3]> {
    let mut fields = text.split_whitespace();
    let one = fields.next()?.parse().ok()?;
    let five = fields.next()?.parse().ok()?;
    let fifteen = fields.next()?.parse().ok()?;
    Some([one, five, fifteen])
}

/// macOS/BSD: `uptime` plus `df`. CPU and memory need platform-specific
/// counters not implemented yet; the notes say so instead of showing zeros.
pub fn parse_unix_fallback(stdout: &str, sampled_at_ms: i64) -> HostMetrics {
    let mut sections = stdout.split(MARKER);
    let uptime_line = sections.next().unwrap_or("");
    let df = sections.next().unwrap_or("");

    // `… load averages: 1.84 1.90 2.01` (macOS) or `load average: 0.12, …`.
    let load = uptime_line
        .rsplit_once("load average")
        .map(|(_, tail)| tail.trim_start_matches(['s', ':', ' ']))
        .and_then(|tail| {
            let values: Vec<f64> = tail
                .split([',', ' '])
                .map(str::trim)
                .filter(|piece| !piece.is_empty())
                .filter_map(|piece| piece.parse().ok())
                .collect();
            (values.len() >= 3).then(|| [values[0], values[1], values[2]])
        });

    HostMetrics {
        sampled_at_ms,
        cpu_percent: None,
        memory: None,
        load,
        uptime_seconds: None,
        disks: parse_df(df),
        notes: vec![
            "CPU and memory sampling is implemented for Linux and Windows; this \
             platform reports load and disks."
                .to_string(),
        ],
    }
}

/// Windows: one CIM JSON document.
pub fn parse_windows(stdout: &str, sampled_at_ms: i64) -> HostMetrics {
    let mut notes = Vec::new();

    let value: Option<serde_json::Value> = serde_json::from_str(stdout.trim()).ok();
    let Some(value) = value else {
        return HostMetrics {
            sampled_at_ms,
            cpu_percent: None,
            memory: None,
            load: None,
            uptime_seconds: None,
            disks: Vec::new(),
            notes: vec!["The host's PowerShell answer could not be parsed.".to_string()],
        };
    };

    let cpu_percent = value.get("cpu").and_then(|cpu| cpu.as_f64());

    let memory = value.get("os").and_then(|os| {
        let total_kb = os.get("TotalVisibleMemorySize")?.as_u64()?;
        let available_kb = os.get("FreePhysicalMemory")?.as_u64()?;
        if total_kb == 0 {
            return None;
        }
        Some(MemoryInfo {
            total_kb,
            available_kb,
            used_percent: total_kb.saturating_sub(available_kb) as f64 / total_kb as f64 * 100.0,
        })
    });

    // CIM datetimes serialize as `/Date(1697049600000)/` - epoch milliseconds.
    let uptime_seconds = value
        .get("os")
        .and_then(|os| os.get("LastBootUpTime"))
        .and_then(|boot| boot.as_str())
        .and_then(parse_cim_date_ms)
        .and_then(|boot_ms| {
            (sampled_at_ms > boot_ms).then(|| ((sampled_at_ms - boot_ms) / 1000) as u64)
        });

    // ConvertTo-Json unwraps one-element arrays even under @(); tolerate both.
    let disks = match value.get("disks") {
        Some(serde_json::Value::Array(items)) => items.iter().filter_map(windows_disk).collect(),
        Some(item @ serde_json::Value::Object(_)) => {
            windows_disk(item).into_iter().collect()
        }
        _ => Vec::new(),
    };

    if cpu_percent.is_none() {
        notes.push("The host did not report a CPU load figure.".to_string());
    }

    HostMetrics {
        sampled_at_ms,
        cpu_percent,
        memory,
        load: None,
        uptime_seconds,
        disks,
        notes,
    }
}

fn windows_disk(value: &serde_json::Value) -> Option<DiskInfo> {
    let mount = value.get("DeviceID")?.as_str()?.to_string();
    let total = value.get("Size")?.as_u64()?;
    let free = value.get("FreeSpace")?.as_u64()?;
    if total == 0 {
        return None;
    }
    let used = total.saturating_sub(free);
    Some(DiskInfo {
        mount,
        total_kb: total / 1024,
        used_kb: used / 1024,
        used_percent: used as f64 / total as f64 * 100.0,
    })
}

fn parse_cim_date_ms(text: &str) -> Option<i64> {
    let digits: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_comes_from_the_delta() {
        let first = CpuTimes { busy: 1_000, total: 10_000 };
        let second = CpuTimes { busy: 1_500, total: 11_000 };
        // 500 busy of 1000 elapsed jiffies = 50%.
        assert_eq!(cpu_percent(first, second), Some(50.0));
    }

    #[test]
    fn a_rebooted_counter_yields_no_reading_rather_than_a_negative() {
        let before = CpuTimes { busy: 9_000, total: 90_000 };
        let after_reboot = CpuTimes { busy: 10, total: 100 };
        assert_eq!(cpu_percent(before, after_reboot), None);
        // Identical readings (no time elapsed) are also not a percentage.
        assert_eq!(cpu_percent(before, before), None);
    }

    #[test]
    fn parses_proc_stat_counting_iowait_as_idle() {
        //                 user  nice system idle  iowait irq softirq steal
        let stat = "cpu  1000 50   300    8000  200    10  40      0\ncpu0 ...\n";
        let times = parse_proc_stat(stat).unwrap();
        assert_eq!(times.total, 9600);
        assert_eq!(times.busy, 1400); // total minus idle(8000) minus iowait(200)
    }

    #[test]
    fn meminfo_prefers_memavailable() {
        let text = "MemTotal:       16384000 kB\nMemFree:         1000000 kB\n\
                    MemAvailable:    8192000 kB\nBuffers:          500000 kB\n";
        let memory = parse_meminfo(text).unwrap();
        assert_eq!(memory.total_kb, 16_384_000);
        assert_eq!(memory.available_kb, 8_192_000);
        assert!((memory.used_percent - 50.0).abs() < 0.01);
    }

    #[test]
    fn meminfo_falls_back_for_ancient_kernels() {
        // No MemAvailable, as on kernels before 3.14.
        let text = "MemTotal:       4000000 kB\nMemFree:         500000 kB\n\
                    Buffers:          250000 kB\nCached:          1250000 kB\n";
        let memory = parse_meminfo(text).unwrap();
        assert_eq!(memory.available_kb, 2_000_000);
        assert!((memory.used_percent - 50.0).abs() < 0.01);
    }

    #[test]
    fn df_keeps_real_disks_and_drops_ram_mirrors() {
        let text = "\
Filesystem     1024-blocks      Used Available Capacity Mounted on\n\
/dev/nvme0n1p2   487652352 123456789 339406835      27% /\n\
tmpfs              8137216         0   8137216       0% /dev/shm\n\
/dev/sda1          1000000    250000    750000      25% /mnt/space disk\n";
        let disks = parse_df(text);
        assert_eq!(disks.len(), 2);
        assert_eq!(disks[0].mount, "/");
        assert_eq!(disks[0].total_kb, 487_652_352);
        // A mount point with a space survives intact.
        assert_eq!(disks[1].mount, "/mnt/space disk");
        assert!((disks[1].used_percent - 25.0).abs() < 0.01);
    }

    #[test]
    fn parses_the_full_linux_compound_output() {
        let stdout = "\
cpu  100 0 100 700 100 0 0 0\n\
---PAROLA---\n\
MemTotal:       1000000 kB\nMemAvailable:    600000 kB\n\
---PAROLA---\n\
Filesystem 1024-blocks Used Available Capacity Mounted on\n\
/dev/sda1 1000000 400000 600000 40% /\n\
---PAROLA---\n\
12345.67 23456.78\n\
---PAROLA---\n\
0.52 0.44 0.30 1/234 5678\n";

        // First sample: no previous reading, so no CPU yet - and a note says so.
        let (first, current) = parse_linux(stdout, None, 1_000);
        assert_eq!(first.cpu_percent, None);
        assert!(first.notes[0].contains("second sample"));
        assert_eq!(first.uptime_seconds, Some(12_345));
        assert_eq!(first.load, Some([0.52, 0.44, 0.30]));
        assert_eq!(first.memory.as_ref().unwrap().total_kb, 1_000_000);
        assert_eq!(first.disks.len(), 1);

        // Second sample against the stored reading produces a percentage.
        let previous = current.unwrap();
        let later = stdout.replace(
            "cpu  100 0 100 700 100 0 0 0",
            "cpu  200 0 200 1100 100 0 0 0",
        );
        let (second, _) = parse_linux(&later, Some(previous), 2_000);
        // 200 more busy jiffies of 600 elapsed = 33.3%.
        let cpu = second.cpu_percent.unwrap();
        assert!((cpu - 33.333).abs() < 0.1, "got {cpu}");
        assert!(second.notes.is_empty());
    }

    #[test]
    fn parses_the_windows_json_document() {
        let stdout = r#"{
  "os": { "TotalVisibleMemorySize": 16712204, "FreePhysicalMemory": 8356102,
          "LastBootUpTime": "\/Date(1000000)\/" },
  "cpu": 12.5,
  "disks": [
    { "DeviceID": "C:", "Size": 255953203200, "FreeSpace": 63988300800 }
  ]
}"#;
        let metrics = parse_windows(stdout, 87_400_000);
        assert_eq!(metrics.cpu_percent, Some(12.5));
        let memory = metrics.memory.unwrap();
        assert_eq!(memory.total_kb, 16_712_204);
        assert!((memory.used_percent - 50.0).abs() < 0.01);
        // (87_400_000 - 1_000_000) ms = 86_400 s - one day of uptime.
        assert_eq!(metrics.uptime_seconds, Some(86_400));
        assert_eq!(metrics.disks.len(), 1);
        assert_eq!(metrics.disks[0].mount, "C:");
        assert!((metrics.disks[0].used_percent - 75.0).abs() < 0.01);
        assert!(metrics.load.is_none());
    }

    #[test]
    fn windows_json_with_a_single_disk_object_still_parses() {
        // ConvertTo-Json unwraps one-element arrays; the parser must not care.
        let stdout = r#"{ "os": null, "cpu": null,
            "disks": { "DeviceID": "C:", "Size": 1024000, "FreeSpace": 512000 } }"#;
        let metrics = parse_windows(stdout, 0);
        assert_eq!(metrics.disks.len(), 1);
        assert!(metrics.cpu_percent.is_none());
        assert!(!metrics.notes.is_empty());
    }

    #[test]
    fn garbage_from_the_host_is_a_note_not_a_panic() {
        let metrics = parse_windows("PowerShell is hosed", 0);
        assert!(metrics.disks.is_empty());
        assert!(metrics.notes[0].contains("could not be parsed"));
    }

    #[test]
    fn the_unix_fallback_reads_load_from_uptime() {
        let stdout = "10:15  up 3 days,  2:04, 2 users, load averages: 1.84 1.90 2.01\n\
---PAROLA---\n\
Filesystem 1024-blocks Used Available Capacity Mounted on\n\
/dev/disk3s5 971350180 850000000 121350180 88% /\n";
        let metrics = parse_unix_fallback(stdout, 0);
        assert_eq!(metrics.load, Some([1.84, 1.90, 2.01]));
        assert_eq!(metrics.disks.len(), 1);
        assert!(metrics.cpu_percent.is_none());
        assert!(metrics.notes[0].contains("load and disks"));

        // Linux wording of the same line, with commas.
        let linuxish = "10:15:01 up 3 days, 2 users, load average: 0.12, 0.20, 0.31\n";
        let metrics = parse_unix_fallback(linuxish, 0);
        assert_eq!(metrics.load, Some([0.12, 0.20, 0.31]));
    }

    #[test]
    fn an_unknown_os_is_refused_rather_than_guessed() {
        assert!(sample_command(OsFamily::Unknown).is_err());
    }
}
