//! What runs a Linux host: its init system, and whether it is a container.
//!
//! Read once at connect, in one round trip, so Services and Power can pick the
//! right commands - or say plainly why there are none - instead of failing
//! with systemctl's "System has not been booted with systemd".

use serde::Serialize;

use super::client::Session;
use super::power::sh_c;
use super::OsFamily;

/// Which service manager a Linux host runs. Other OS families are `Native`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InitSystem {
    Systemd,
    OpenRc,
    /// Plain `/etc/init.d` scripts driven by `service`.
    SysV,
    /// Nothing supervises services - typical of a minimal container.
    None,
    /// Not Linux: Windows' SCM, launchd, rc.d.
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerKind {
    Docker,
    Podman,
    Lxc,
    Kubernetes,
    Nspawn,
    Other,
}

impl ContainerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Docker => "Docker",
            Self::Podman => "Podman",
            Self::Lxc => "LXC",
            Self::Kubernetes => "Kubernetes",
            Self::Nspawn => "systemd-nspawn",
            Self::Other => "container",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    pub init: InitSystem,
    pub container: Option<ContainerKind>,
    /// PID 1's name, e.g. `systemd`, `sshd`, `tini`. Empty when unreadable.
    pub pid1: String,
    /// Whether a `shutdown` command exists; BusyBox systems only have `reboot`.
    pub has_shutdown: bool,
}

/// PID 1 names that are real inits, able to reboot what they run.
const INIT_PROCESSES: &[&str] = &["systemd", "init", "openrc-init", "runit", "s6-svscan"];

impl Platform {
    pub fn native() -> Self {
        Self { init: InitSystem::Native, container: None, pid1: String::new(), has_shutdown: true }
    }

    /// Why power actions cannot work here, if they cannot. A container whose
    /// PID 1 is an application has no init to reboot; its runtime restarts it.
    pub fn power_refusal(&self) -> Option<String> {
        let kind = self.container?;
        if INIT_PROCESSES.contains(&self.pid1.as_str()) {
            return None;
        }
        let pid1 = if self.pid1.is_empty() { "an application".to_string() } else { format!("`{}`", self.pid1) };
        Some(format!(
            "This is a {} container whose main process is {pid1}, not an init system, so it \
             cannot shut down or reboot itself. Restart it from the machine running it, \
             e.g. `docker restart <container>`.",
            kind.label()
        ))
    }

    /// Whether a delayed shutdown can be scheduled and cancelled.
    pub fn can_schedule_power(&self) -> bool {
        self.has_shutdown
    }
}

/// One POSIX-sh round trip. `sh -c` so a fish or zsh login shell reads it the same.
const PROBE: &str = "\
if [ -d /run/systemd/system ]; then echo INIT=systemd; \
elif command -v rc-service >/dev/null 2>&1; then echo INIT=openrc; \
elif command -v service >/dev/null 2>&1 || [ -d /etc/init.d ]; then echo INIT=sysv; \
else echo INIT=none; fi; \
[ -f /.dockerenv ] && echo CONTAINER=docker; \
[ -f /run/.containerenv ] && echo CONTAINER=podman; \
[ -n \"${container:-}\" ] && echo \"CONTAINER=$container\"; \
command -v systemd-detect-virt >/dev/null 2>&1 && echo \"VIRT=$(systemd-detect-virt --container 2>/dev/null)\"; \
echo \"CGROUP=$(grep -oaE 'docker|kubepods|libpod|lxc' /proc/1/cgroup 2>/dev/null | head -n 1)\"; \
echo \"PID1=$(cat /proc/1/comm 2>/dev/null)\"; \
command -v shutdown >/dev/null 2>&1 && echo SHUTDOWN=yes; \
true";

pub async fn detect(session: &Session, os: OsFamily) -> Platform {
    if os != OsFamily::Linux {
        return Platform::native();
    }
    match session.exec(&sh_c(PROBE), None).await {
        Ok(output) => parse(&output.stdout),
        // Keep the pre-detection assumption rather than disabling a working host.
        Err(_) => Platform { init: InitSystem::Systemd, container: None, pid1: String::new(), has_shutdown: true },
    }
}

/// Read the probe's `KEY=value` lines. Pure.
fn parse(stdout: &str) -> Platform {
    let mut init = InitSystem::None;
    let mut explicit: Option<ContainerKind> = None;
    let mut virt: Option<ContainerKind> = None;
    let mut cgroup: Option<ContainerKind> = None;
    let mut pid1 = String::new();
    let mut has_shutdown = false;

    for line in stdout.lines() {
        let Some((key, value)) = line.trim().split_once('=') else { continue };
        let value = value.trim();
        match key {
            "INIT" => {
                init = match value {
                    "systemd" => InitSystem::Systemd,
                    "openrc" => InitSystem::OpenRc,
                    "sysv" => InitSystem::SysV,
                    _ => InitSystem::None,
                }
            }
            // The first marker file wins; `$container` only fills a gap.
            "CONTAINER" if explicit.is_none() => explicit = container_kind(value),
            "VIRT" => virt = container_kind(value),
            "CGROUP" => cgroup = container_kind(value),
            "PID1" => pid1 = value.to_string(),
            "SHUTDOWN" => has_shutdown = value == "yes",
            _ => {}
        }
    }

    Platform { init, container: explicit.or(virt).or(cgroup), pid1, has_shutdown }
}

fn container_kind(value: &str) -> Option<ContainerKind> {
    match value {
        "" | "none" => None,
        "docker" => Some(ContainerKind::Docker),
        "podman" | "libpod" => Some(ContainerKind::Podman),
        "lxc" | "lxc-libvirt" => Some(ContainerKind::Lxc),
        "kubepods" => Some(ContainerKind::Kubernetes),
        "systemd-nspawn" => Some(ContainerKind::Nspawn),
        _ => Some(ContainerKind::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_systemd_machine_is_no_container() {
        let platform = parse("INIT=systemd\nVIRT=none\nCGROUP=\nPID1=systemd\nSHUTDOWN=yes\n");
        assert_eq!(
            platform,
            Platform { init: InitSystem::Systemd, container: None, pid1: "systemd".into(), has_shutdown: true }
        );
        assert_eq!(platform.power_refusal(), None);
    }

    #[test]
    fn a_debian_docker_container_runs_sysv_scripts_under_sshd() {
        let platform = parse("INIT=sysv\nCONTAINER=docker\nCGROUP=\nPID1=sshd\nSHUTDOWN=yes\n");
        assert_eq!(platform.init, InitSystem::SysV);
        assert_eq!(platform.container, Some(ContainerKind::Docker));
        let refusal = platform.power_refusal().unwrap();
        assert!(refusal.contains("Docker container whose main process is `sshd`"), "{refusal}");
    }

    #[test]
    fn a_minimal_alpine_container_has_no_init_and_no_shutdown() {
        let platform = parse("INIT=none\nCONTAINER=docker\nCGROUP=\nPID1=sleep\n");
        assert_eq!(platform.init, InitSystem::None);
        assert!(!platform.has_shutdown);
        assert!(!platform.can_schedule_power());
    }

    #[test]
    fn an_lxc_system_container_with_an_init_may_power_itself() {
        let platform = parse("INIT=openrc\nCONTAINER=lxc\nCGROUP=lxc\nPID1=init\n");
        assert_eq!(platform.container, Some(ContainerKind::Lxc));
        assert_eq!(platform.power_refusal(), None);
    }

    #[test]
    fn podman_is_told_apart_from_docker() {
        let platform = parse("INIT=none\nCONTAINER=podman\nCONTAINER=oci\nCGROUP=libpod\nPID1=bash\n");
        assert_eq!(platform.container, Some(ContainerKind::Podman));
    }

    #[test]
    fn cgroup_markers_fill_in_when_nothing_else_says() {
        assert_eq!(parse("INIT=none\nCGROUP=kubepods\nPID1=node\n").container, Some(ContainerKind::Kubernetes));
        assert_eq!(parse("INIT=systemd\nVIRT=systemd-nspawn\nPID1=systemd\n").container, Some(ContainerKind::Nspawn));
    }

    #[test]
    fn garbage_reads_as_unknown_rather_than_systemd() {
        let platform = parse("fish: Unknown command\n");
        assert_eq!(platform.init, InitSystem::None);
        assert_eq!(platform.container, None);
    }
}
