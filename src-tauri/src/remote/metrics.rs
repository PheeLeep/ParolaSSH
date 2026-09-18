//! One-round-trip performance sampling.
//!
//! Each sample is one compound command over the existing session, parsed here
//! into numbers the pane can chart. Nothing is installed or privileged.
//!
//! CPU percentage, network and disk I/O rates are deltas between two cumulative
//! readings, so the previous ones are kept on the session
//! (`LiveSession::prev_readings`) and the first sample reports "no reading
//! yet" rather than sleeping inside the command.
//!
//! macOS and BSD have no `/proc` and get a partial (load, disks, network) with
//! a note. Windows reports through CIM as JSON; its `LoadPercentage` is already
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
    /// Summed across physical interfaces. `None` until there are two readings.
    pub network: Option<NetworkRate>,
    /// Summed across whole disks. `None` until there are two readings.
    pub disk_io: Option<DiskIo>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRate {
    /// Physical interfaces only.
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    /// Physical first, then virtual, each in the host's own order.
    pub interfaces: Vec<InterfaceRate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceRate {
    pub name: String,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    /// Relays traffic a physical NIC also carries, so it is left out of the totals.
    pub is_virtual: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskIo {
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
    pub devices: Vec<DeviceIo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIo {
    pub name: String,
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
}

/// Cumulative CPU counters from one `/proc/stat` reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    pub busy: u64,
    pub total: u64,
}

/// Cumulative byte counters for one interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCounters {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Every interface's counters, stamped with when they were read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetCounters {
    pub at_ms: i64,
    pub interfaces: Vec<InterfaceCounters>,
}

/// Cumulative byte counters for one disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCounters {
    pub name: String,
    pub read_bytes: u64,
    pub write_bytes: u64,
}

/// Every disk's counters, stamped with when they were read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskCounters {
    pub at_ms: i64,
    pub devices: Vec<DeviceCounters>,
}

/// The counters one sample leaves behind for the next one's deltas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Readings {
    pub cpu: Option<CpuTimes>,
    pub net: Option<NetCounters>,
    pub disk: Option<DiskCounters>,
}

/// The whole Linux sample in one exec.
const LINUX_COMMAND: &str = "cat /proc/stat; echo ---PAROLA---; cat /proc/meminfo; \
     echo ---PAROLA---; df -P -k; echo ---PAROLA---; cat /proc/uptime; \
     echo ---PAROLA---; cat /proc/loadavg; echo ---PAROLA---; cat /proc/net/dev; \
     echo ---PAROLA---; cat /proc/diskstats; echo ---PAROLA---; ls /sys/block";

/// What a /proc-less Unix can still answer.
const UNIX_FALLBACK_COMMAND: &str = "uptime; echo ---PAROLA---; df -P -k; \
     echo ---PAROLA---; netstat -ibn; \
     echo ---PAROLA---; ioreg -c IOBlockStorageDriver -r -w0 2>/dev/null";

/// One PowerShell invocation, JSON out, so parsing does not depend on the
/// display locale. `LoadPercentage` is instantaneous - no delta needed.
const WINDOWS_COMMAND: &str = "powershell -NoProfile -NonInteractive -Command \
    \"@{ os = Get-CimInstance Win32_OperatingSystem | Select-Object \
    TotalVisibleMemorySize,FreePhysicalMemory,LastBootUpTime; \
    cpu = (Get-CimInstance Win32_Processor | Measure-Object -Property \
    LoadPercentage -Average).Average; \
    disks = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | \
    Select-Object DeviceID,Size,FreeSpace); \
    net = @(Get-NetAdapterStatistics -ErrorAction SilentlyContinue | \
    Select-Object Name,ReceivedBytes,SentBytes); \
    diskio = @(Get-CimInstance Win32_PerfRawData_PerfDisk_PhysicalDisk | \
    Select-Object Name,DiskReadBytesPersec,DiskWriteBytesPersec) } | \
    ConvertTo-Json -Depth 3\"";

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

    // After the exec, so the lock is never held across an await.
    let previous = live.prev_readings();
    let (metrics, current) = match live.os {
        OsFamily::Linux => parse_linux(&output.stdout, &previous, sampled_at_ms),
        OsFamily::Windows => parse_windows(&output.stdout, &previous, sampled_at_ms),
        _ => parse_unix_fallback(&output.stdout, &previous, sampled_at_ms),
    };
    // A failed read keeps the older reading: the next delta just spans longer.
    live.set_prev_readings(Readings {
        cpu: current.cpu.or(previous.cpu),
        net: current.net.or(previous.net),
        disk: current.disk.or(previous.disk),
    });

    Ok(metrics)
}

/// Parse the Linux compound output. Pure; returns the fresh counters so the
/// caller can store them for the next delta.
pub fn parse_linux(
    stdout: &str,
    previous: &Readings,
    sampled_at_ms: i64,
) -> (HostMetrics, Readings) {
    let mut sections = stdout.split(MARKER);
    let stat = sections.next().unwrap_or("");
    let meminfo = sections.next().unwrap_or("");
    let df = sections.next().unwrap_or("");
    let uptime = sections.next().unwrap_or("");
    let loadavg = sections.next().unwrap_or("");
    let net_dev = sections.next().unwrap_or("");
    let diskstats = sections.next().unwrap_or("");
    let block = sections.next().unwrap_or("");

    let current = Readings {
        cpu: parse_proc_stat(stat),
        net: parse_proc_net_dev(net_dev, sampled_at_ms),
        disk: parse_diskstats(diskstats, block, sampled_at_ms),
    };
    let cpu_percent = match (previous.cpu, current.cpu) {
        (Some(previous), Some(current)) => cpu_percent(previous, current),
        _ => None,
    };

    let mut notes = Vec::new();
    if cpu_percent.is_none() {
        notes.push(
            "CPU and network rates need two readings; they appear from the second sample on."
                .to_string(),
        );
    }
    let network = network_between(previous.net.as_ref(), current.net.as_ref(), &mut notes);
    let disk_io = disk_io_between(previous.disk.as_ref(), current.disk.as_ref(), &mut notes);

    (
        HostMetrics {
            sampled_at_ms,
            cpu_percent,
            memory: parse_meminfo(meminfo),
            load: parse_loadavg(loadavg),
            uptime_seconds: parse_proc_uptime(uptime),
            disks: parse_df(df),
            network,
            disk_io,
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

/// Bytes per second per interface between two readings. An interface that is
/// new, or whose counter shrank (reboot, re-created device), sits this sample
/// out rather than reporting a negative.
pub fn network_rate(previous: &NetCounters, current: &NetCounters) -> Option<NetworkRate> {
    let elapsed_ms = current.at_ms - previous.at_ms;
    if elapsed_ms <= 0 {
        return None;
    }
    let seconds = elapsed_ms as f64 / 1000.0;

    let mut interfaces: Vec<InterfaceRate> = current
        .interfaces
        .iter()
        .filter_map(|now| {
            let before = previous.interfaces.iter().find(|before| before.name == now.name)?;
            if now.rx_bytes < before.rx_bytes || now.tx_bytes < before.tx_bytes {
                return None;
            }
            Some(InterfaceRate {
                name: now.name.clone(),
                rx_bytes_per_sec: (now.rx_bytes - before.rx_bytes) as f64 / seconds,
                tx_bytes_per_sec: (now.tx_bytes - before.tx_bytes) as f64 / seconds,
                is_virtual: is_virtual_interface(&now.name),
            })
        })
        .collect();
    if interfaces.is_empty() {
        return None;
    }
    interfaces.sort_by_key(|interface| interface.is_virtual);

    let physical = interfaces.iter().filter(|interface| !interface.is_virtual);
    let (rx, tx) = physical.fold((0.0, 0.0), |(rx, tx), interface| {
        (rx + interface.rx_bytes_per_sec, tx + interface.tx_bytes_per_sec)
    });

    Some(NetworkRate {
        rx_bytes_per_sec: rx,
        tx_bytes_per_sec: tx,
        interfaces,
    })
}

/// The rate for this sample, noting a host that has no counters at all.
fn network_between(
    previous: Option<&NetCounters>,
    current: Option<&NetCounters>,
    notes: &mut Vec<String>,
) -> Option<NetworkRate> {
    match (previous, current) {
        (Some(previous), Some(current)) => network_rate(previous, current),
        (_, None) => {
            notes.push("The host did not report network counters.".to_string());
            None
        }
        _ => None,
    }
}

/// Loopback, and per-container or per-VM ends that would flood the list.
fn is_ignored_interface(name: &str) -> bool {
    const PREFIXES: &[&str] = &["lo", "veth", "cali", "tap", "vnet"];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// Bridges and tunnels relay traffic a physical NIC also counts, so they are
/// listed but not summed.
fn is_virtual_interface(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "docker", "br-", "virbr", "cni", "flannel", "vxlan", "tun", "wg", "tailscale", "zt",
        "utun", "gif", "stf", "bridge", "vmnet", "vEthernet",
    ];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// Collects one interface, skipping ignored ones and ones that never carried a
/// byte (down NICs, idle placeholders like macOS `gif0`).
fn push_interface(interfaces: &mut Vec<InterfaceCounters>, name: &str, rx_bytes: u64, tx_bytes: u64) {
    if is_ignored_interface(name) || (rx_bytes == 0 && tx_bytes == 0) {
        return;
    }
    interfaces.push(InterfaceCounters {
        name: name.to_string(),
        rx_bytes,
        tx_bytes,
    });
}

/// `/proc/net/dev`: `iface: rx_bytes … (8 rx fields) tx_bytes …`.
fn parse_proc_net_dev(text: &str, at_ms: i64) -> Option<NetCounters> {
    let mut interfaces = Vec::new();

    // The colon can touch the first number (`eth0:123`), so split on it.
    for (name, rest) in text.lines().filter_map(|line| line.split_once(':')) {
        let fields: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|field| field.parse().ok())
            .collect();
        if fields.len() < 9 {
            continue;
        }
        push_interface(&mut interfaces, name.trim(), fields[0], fields[8]);
    }

    Some(NetCounters { at_ms, interfaces }).filter(|counters| !counters.interfaces.is_empty())
}

/// `netstat -ibn` on macOS/BSD. Columns vary by platform and rows without an
/// address are shorter, so byte columns are located from the header and
/// indexed from the end of each row.
fn parse_netstat_ib(text: &str, at_ms: i64) -> Option<NetCounters> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next()?.split_whitespace().collect();
    let from_end = |name: &str| {
        header
            .iter()
            .position(|column| *column == name)
            .map(|index| header.len() - index)
    };
    let rx_back = from_end("Ibytes")?;
    let tx_back = from_end("Obytes")?;

    let mut interfaces = Vec::new();

    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // Address rows repeat the counters of their `<Link#n>` row.
        let is_link_row = fields.get(2).is_some_and(|network| network.starts_with("<Link"));
        if !is_link_row || fields.len() < rx_back.max(tx_back) + 3 {
            continue;
        }
        let (Ok(rx), Ok(tx)) = (
            fields[fields.len() - rx_back].parse::<u64>(),
            fields[fields.len() - tx_back].parse::<u64>(),
        ) else {
            continue;
        };
        // macOS marks a down interface with a trailing `*`.
        push_interface(&mut interfaces, fields[0].trim_end_matches('*'), rx, tx);
    }

    Some(NetCounters { at_ms, interfaces }).filter(|counters| !counters.interfaces.is_empty())
}

/// Bytes per second per disk between two readings, with the same rules as
/// `network_rate` for new or reset devices.
pub fn disk_io_rate(previous: &DiskCounters, current: &DiskCounters) -> Option<DiskIo> {
    let elapsed_ms = current.at_ms - previous.at_ms;
    if elapsed_ms <= 0 {
        return None;
    }
    let seconds = elapsed_ms as f64 / 1000.0;

    let devices: Vec<DeviceIo> = current
        .devices
        .iter()
        .filter_map(|now| {
            let before = previous.devices.iter().find(|before| before.name == now.name)?;
            if now.read_bytes < before.read_bytes || now.write_bytes < before.write_bytes {
                return None;
            }
            Some(DeviceIo {
                name: now.name.clone(),
                read_bytes_per_sec: (now.read_bytes - before.read_bytes) as f64 / seconds,
                write_bytes_per_sec: (now.write_bytes - before.write_bytes) as f64 / seconds,
            })
        })
        .collect();
    if devices.is_empty() {
        return None;
    }

    Some(DiskIo {
        read_bytes_per_sec: devices.iter().map(|device| device.read_bytes_per_sec).sum(),
        write_bytes_per_sec: devices.iter().map(|device| device.write_bytes_per_sec).sum(),
        devices,
    })
}

fn disk_io_between(
    previous: Option<&DiskCounters>,
    current: Option<&DiskCounters>,
    notes: &mut Vec<String>,
) -> Option<DiskIo> {
    match (previous, current) {
        (Some(previous), Some(current)) => disk_io_rate(previous, current),
        (_, None) => {
            notes.push("The host did not report disk I/O counters.".to_string());
            None
        }
        _ => None,
    }
}

/// Collects one disk, skipping ones that never moved a byte.
fn push_device(devices: &mut Vec<DeviceCounters>, name: &str, read_bytes: u64, write_bytes: u64) {
    if read_bytes == 0 && write_bytes == 0 {
        return;
    }
    devices.push(DeviceCounters {
        name: name.to_string(),
        read_bytes,
        write_bytes,
    });
}

/// `/proc/diskstats`, whole disks only: `/sys/block` lists them without their
/// partitions. Loop, RAM and device-mapper/RAID layers are dropped because
/// their I/O is already counted on the disks underneath. Sectors are always
/// 512 bytes here, whatever the hardware uses.
fn parse_diskstats(text: &str, block: &str, at_ms: i64) -> Option<DiskCounters> {
    const LAYERED: &[&str] = &["loop", "ram", "zram", "dm-", "md", "sr", "fd", "nbd"];
    let whole: Vec<&str> = block
        .split_whitespace()
        .filter(|name| !LAYERED.iter().any(|prefix| name.starts_with(prefix)))
        .collect();

    let mut devices = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 || !whole.contains(&fields[2]) {
            continue;
        }
        let (Ok(read), Ok(written)) = (fields[5].parse::<u64>(), fields[9].parse::<u64>()) else {
            continue;
        };
        push_device(&mut devices, fields[2], read * 512, written * 512);
    }

    Some(DiskCounters { at_ms, devices }).filter(|counters| !counters.devices.is_empty())
}

/// macOS `ioreg`: each storage driver carries a `Statistics` dictionary with
/// `"Bytes (Read)"` and `"Bytes (Write)"`. No reliable disk name comes with it,
/// so the drivers are summed into one entry.
fn parse_ioreg_storage(text: &str, at_ms: i64) -> Option<DiskCounters> {
    let total = |key: &str| -> Option<u64> {
        let values: Vec<u64> = text
            .match_indices(key)
            .filter_map(|(index, _)| {
                let digits: String = text[index + key.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                digits.parse().ok()
            })
            .collect();
        (!values.is_empty()).then(|| values.iter().sum())
    };

    let mut devices = Vec::new();
    push_device(
        &mut devices,
        "all disks",
        total("\"Bytes (Read)\"=")?,
        total("\"Bytes (Write)\"=")?,
    );
    Some(DiskCounters { at_ms, devices }).filter(|counters| !counters.devices.is_empty())
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

/// macOS/BSD: `uptime`, `df` and `netstat`. CPU and memory need
/// platform-specific counters not implemented yet; the notes say so instead
/// of showing zeros.
pub fn parse_unix_fallback(
    stdout: &str,
    previous: &Readings,
    sampled_at_ms: i64,
) -> (HostMetrics, Readings) {
    let mut sections = stdout.split(MARKER);
    let uptime_line = sections.next().unwrap_or("");
    let df = sections.next().unwrap_or("");
    let netstat = sections.next().unwrap_or("");
    let ioreg = sections.next().unwrap_or("");

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

    let mut notes = vec![
        "CPU and memory sampling is implemented for Linux and Windows; this \
         platform reports load, disks, network and disk I/O."
            .to_string(),
    ];
    let current = Readings {
        cpu: None,
        net: parse_netstat_ib(netstat, sampled_at_ms),
        disk: parse_ioreg_storage(ioreg, sampled_at_ms),
    };
    let network = network_between(previous.net.as_ref(), current.net.as_ref(), &mut notes);
    let disk_io = disk_io_between(previous.disk.as_ref(), current.disk.as_ref(), &mut notes);

    (
        HostMetrics {
            sampled_at_ms,
            cpu_percent: None,
            memory: None,
            load,
            uptime_seconds: None,
            disks: parse_df(df),
            network,
            disk_io,
            notes,
        },
        current,
    )
}

/// Windows: one CIM JSON document.
pub fn parse_windows(
    stdout: &str,
    previous: &Readings,
    sampled_at_ms: i64,
) -> (HostMetrics, Readings) {
    let mut notes = Vec::new();

    let value: Option<serde_json::Value> = serde_json::from_str(stdout.trim()).ok();
    let Some(value) = value else {
        let metrics = HostMetrics {
            sampled_at_ms,
            cpu_percent: None,
            memory: None,
            load: None,
            uptime_seconds: None,
            disks: Vec::new(),
            network: None,
            disk_io: None,
            notes: vec!["The host's PowerShell answer could not be parsed.".to_string()],
        };
        return (metrics, Readings::default());
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

    let current = Readings {
        cpu: None,
        net: windows_net(value.get("net"), sampled_at_ms),
        disk: windows_disk_io(value.get("diskio"), sampled_at_ms),
    };
    let network = network_between(previous.net.as_ref(), current.net.as_ref(), &mut notes);
    let disk_io = disk_io_between(previous.disk.as_ref(), current.disk.as_ref(), &mut notes);

    (
        HostMetrics {
            sampled_at_ms,
            cpu_percent,
            memory,
            load: None,
            uptime_seconds,
            disks,
            network,
            disk_io,
            notes,
        },
        current,
    )
}

/// `Get-NetAdapterStatistics` rows. A lone row may be unwrapped from its array.
fn windows_net(value: Option<&serde_json::Value>, at_ms: i64) -> Option<NetCounters> {
    let rows: Vec<&serde_json::Value> = match value? {
        serde_json::Value::Array(items) => items.iter().collect(),
        item @ serde_json::Value::Object(_) => vec![item],
        _ => return None,
    };
    let count = |row: &serde_json::Value, key: &str| -> Option<u64> {
        let value = row.get(key)?;
        value.as_u64().or_else(|| value.as_f64().map(|float| float as u64))
    };

    let mut interfaces = Vec::new();
    for row in rows {
        let (Some(name), Some(rx), Some(tx)) = (
            row.get("Name").and_then(|name| name.as_str()),
            count(row, "ReceivedBytes"),
            count(row, "SentBytes"),
        ) else {
            continue;
        };
        push_interface(&mut interfaces, name, rx, tx);
    }

    Some(NetCounters { at_ms, interfaces }).filter(|counters| !counters.interfaces.is_empty())
}

/// `Win32_PerfRawData_PerfDisk_PhysicalDisk` rows: raw values of the "per sec"
/// counters are cumulative byte counts. `_Total` is dropped in favour of the
/// per-disk rows.
fn windows_disk_io(value: Option<&serde_json::Value>, at_ms: i64) -> Option<DiskCounters> {
    let rows: Vec<&serde_json::Value> = match value? {
        serde_json::Value::Array(items) => items.iter().collect(),
        item @ serde_json::Value::Object(_) => vec![item],
        _ => return None,
    };
    let count = |row: &serde_json::Value, key: &str| -> Option<u64> {
        let value = row.get(key)?;
        value
            .as_u64()
            .or_else(|| value.as_f64().map(|float| float as u64))
            .or_else(|| value.as_str()?.parse().ok())
    };

    let mut devices = Vec::new();
    for row in rows {
        let (Some(name), Some(read), Some(write)) = (
            row.get("Name").and_then(|name| name.as_str()),
            count(row, "DiskReadBytesPersec"),
            count(row, "DiskWriteBytesPersec"),
        ) else {
            continue;
        };
        if name != "_Total" {
            push_device(&mut devices, name, read, write);
        }
    }

    Some(DiskCounters { at_ms, devices }).filter(|counters| !counters.devices.is_empty())
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
0.52 0.44 0.30 1/234 5678\n\
---PAROLA---\n\
Inter-|   Receive                            |  Transmit\n\
 face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n\
    lo: 9999 1 0 0 0 0 0 0 9999 1 0 0 0 0 0 0\n\
  eth0:1000 10 0 0 0 0 0 0 500 5 0 0 0 0 0 0\n\
   wg0: 100 1 0 0 0 0 0 0 100 1 0 0 0 0 0 0\n\
---PAROLA---\n\
   8       0 sda 100 0 2000 0 50 0 1000 0 0 0 0\n\
   8       1 sda1 100 0 2000 0 50 0 1000 0 0 0 0\n\
   7       0 loop0 10 0 80 0 0 0 0 0 0 0 0\n\
---PAROLA---\n\
loop0\nsda\n";

        // First sample: no previous reading, so no CPU yet - and a note says so.
        let (first, current) = parse_linux(stdout, &Readings::default(), 1_000);
        assert_eq!(first.cpu_percent, None);
        assert!(first.network.is_none());
        assert!(first.notes[0].contains("second sample"));
        assert_eq!(first.uptime_seconds, Some(12_345));
        assert_eq!(first.load, Some([0.52, 0.44, 0.30]));
        assert_eq!(first.memory.as_ref().unwrap().total_kb, 1_000_000);
        assert_eq!(first.disks.len(), 1);

        // Second sample against the stored reading produces a percentage.
        let later = stdout
            .replace("cpu  100 0 100 700 100 0 0 0", "cpu  200 0 200 1100 100 0 0 0")
            .replace("eth0:1000 10 0 0 0 0 0 0 500", "eth0:3000 10 0 0 0 0 0 0 1500")
            .replace("sda 100 0 2000 0 50 0 1000", "sda 100 0 4000 0 50 0 1500");
        let (second, _) = parse_linux(&later, &current, 2_000);
        // 200 more busy jiffies of 600 elapsed = 33.3%.
        let cpu = second.cpu_percent.unwrap();
        assert!((cpu - 33.333).abs() < 0.1, "got {cpu}");
        // Loopback is dropped and the tunnel listed but not summed: 2000 bytes
        // down, 1000 up, over one second.
        let network = second.network.unwrap();
        assert_eq!(network.rx_bytes_per_sec, 2000.0);
        assert_eq!(network.tx_bytes_per_sec, 1000.0);
        let names: Vec<&str> = network.interfaces.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["eth0", "wg0"]);
        assert!(network.interfaces[1].is_virtual);
        // Only the whole disk counts, in 512-byte sectors: 2000 and 500 more.
        let disk_io = second.disk_io.unwrap();
        assert_eq!(disk_io.read_bytes_per_sec, 2000.0 * 512.0);
        assert_eq!(disk_io.write_bytes_per_sec, 500.0 * 512.0);
        assert_eq!(disk_io.devices.len(), 1);
        assert!(second.notes.is_empty());
    }

    fn counters(at_ms: i64, rows: &[(&str, u64, u64)]) -> NetCounters {
        NetCounters {
            at_ms,
            interfaces: rows
                .iter()
                .map(|(name, rx, tx)| InterfaceCounters {
                    name: name.to_string(),
                    rx_bytes: *rx,
                    tx_bytes: *tx,
                })
                .collect(),
        }
    }

    #[test]
    fn network_rate_skips_reset_and_new_interfaces() {
        let before = counters(1_000, &[("eth0", 5_000, 5_000), ("eth1", 5_000, 5_000)]);
        assert!(network_rate(&before, &before).is_none(), "no time elapsed");

        // eth1 was re-created (counter shrank) and usb0 is new: only eth0 counts.
        let later = counters(
            3_000,
            &[("eth0", 6_000, 5_500), ("eth1", 10, 10), ("usb0", 900, 900)],
        );
        let rate = network_rate(&before, &later).unwrap();
        assert_eq!(rate.rx_bytes_per_sec, 500.0);
        assert_eq!(rate.tx_bytes_per_sec, 250.0);
        assert_eq!(rate.interfaces.len(), 1);
    }

    #[test]
    fn ioreg_sums_every_storage_driver() {
        let text = r#"
    | "Statistics" = {"Bytes (Read)"=1000,"Operations (Write)"=3,"Bytes (Write)"=200}
    | "Statistics" = {"Bytes (Read)"=500,"Bytes (Write)"=50}
"#;
        let disk = parse_ioreg_storage(text, 7).unwrap();
        assert_eq!(disk.devices[0].read_bytes, 1_500);
        assert_eq!(disk.devices[0].write_bytes, 250);
        assert!(parse_ioreg_storage("", 7).is_none(), "BSD has no ioreg");
    }

    #[test]
    fn netstat_counts_each_interface_once_from_its_link_row() {
        // macOS layout: lo0's link row has no address, so it is one field short.
        let macos = "\
Name  Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll\n\
lo0   16384 <Link#1>                           100     0       9999      100     0       9999     0\n\
en0   1500  <Link#4>    a0:b1:c2:d3:e4:f5     2000     0     100000     1500     0      40000     0\n\
en0   1500  192.168.1     192.168.1.20        2000     -     100000     1500     -      40000     -\n\
utun0 1380  <Link#9>                            50     0       7777       50     0       7777     0\n";
        let parsed = parse_netstat_ib(macos, 0).unwrap();
        assert_eq!(parsed, counters(0, &[("en0", 100_000, 40_000), ("utun0", 7_777, 7_777)]));

        // OpenBSD prints only the byte columns.
        let openbsd = "\
Name    Mtu   Network     Address              Ibytes       Obytes\n\
em0     1500  <Link>      08:00:27:aa:bb:cc    123456       654321\n";
        let parsed = parse_netstat_ib(openbsd, 0).unwrap();
        assert_eq!(parsed, counters(0, &[("em0", 123_456, 654_321)]));
    }

    #[test]
    fn parses_the_windows_json_document() {
        let stdout = r#"{
  "os": { "TotalVisibleMemorySize": 16712204, "FreePhysicalMemory": 8356102,
          "LastBootUpTime": "\/Date(1000000)\/" },
  "cpu": 12.5,
  "disks": [
    { "DeviceID": "C:", "Size": 255953203200, "FreeSpace": 63988300800 }
  ],
  "net": { "Name": "Ethernet 2", "ReceivedBytes": 4000000, "SentBytes": 1000000 },
  "diskio": [
    { "Name": "0 C:", "DiskReadBytesPersec": 5000, "DiskWriteBytesPersec": 7000 },
    { "Name": "_Total", "DiskReadBytesPersec": 5000, "DiskWriteBytesPersec": 7000 }
  ]
}"#;
        let (metrics, current) = parse_windows(stdout, &Readings::default(), 87_400_000);
        assert!(metrics.network.is_none(), "the first sample has no rate yet");
        assert_eq!(
            current.net.unwrap(),
            counters(87_400_000, &[("Ethernet 2", 4_000_000, 1_000_000)])
        );
        let disk = current.disk.unwrap();
        assert_eq!(disk.devices.len(), 1, "_Total is not a disk");
        assert_eq!(disk.devices[0].name, "0 C:");
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
        let (metrics, _) = parse_windows(stdout, &Readings::default(), 0);
        assert_eq!(metrics.disks.len(), 1);
        assert!(metrics.cpu_percent.is_none());
        assert!(!metrics.notes.is_empty());
    }

    #[test]
    fn garbage_from_the_host_is_a_note_not_a_panic() {
        let (metrics, _) = parse_windows("PowerShell is hosed", &Readings::default(), 0);
        assert!(metrics.disks.is_empty());
        assert!(metrics.notes[0].contains("could not be parsed"));
    }

    #[test]
    fn the_unix_fallback_reads_load_from_uptime() {
        let stdout = "10:15  up 3 days,  2:04, 2 users, load averages: 1.84 1.90 2.01\n\
---PAROLA---\n\
Filesystem 1024-blocks Used Available Capacity Mounted on\n\
/dev/disk3s5 971350180 850000000 121350180 88% /\n";
        let (metrics, _) = parse_unix_fallback(stdout, &Readings::default(), 0);
        assert_eq!(metrics.load, Some([1.84, 1.90, 2.01]));
        assert_eq!(metrics.disks.len(), 1);
        assert!(metrics.cpu_percent.is_none());
        assert!(metrics.notes[0].contains("network and disk I/O"));

        // Linux wording of the same line, with commas.
        let linuxish = "10:15:01 up 3 days, 2 users, load average: 0.12, 0.20, 0.31\n";
        let (metrics, _) = parse_unix_fallback(linuxish, &Readings::default(), 0);
        assert_eq!(metrics.load, Some([0.12, 0.20, 0.31]));
    }

    #[test]
    fn an_unknown_os_is_refused_rather_than_guessed() {
        assert!(sample_command(OsFamily::Unknown).is_err());
    }
}
