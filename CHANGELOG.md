# Changelog

All notable changes to ParolaSSH. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0] - Unreleased

First public release, for **Linux and Windows**. macOS builds are not provided:
the code has macOS branches, but no Mac has ever run them.

### Connections
- Saved hosts with groups, tags and search; import from `~/.ssh/config`
- Password, private key, SSH agent, or `none` (Tailscale SSH) authentication
- Host key verification during key exchange, before any password is sent
- Jump hosts (`ProxyJump`) through other saved connections
- Port probe that tells refused, timed out and not-SSH apart, and lists the
  server's auth methods
- Optional detailed connection status: one line naming the current step
  (jump host, dial, host key, encryption, sign-in)
- 30-second heartbeat with four status states

### Per-host panes
- **Terminal** - real PTY, up to 8 renameable tabs per host, per-tab font,
  broadcast input
- **Services** - systemd, OpenRC, SysV init scripts, and the Windows service
  manager: start, stop, restart, history and live follow
- **Performance** - CPU, memory, load, disks, network and disk I/O, sampled
  every 0.5-30 s
- **Tasks** - built-in and your own one-click commands, with a danger check
  before anything destructive
- **Security** - handshake crypto, `sshd -T` posture, listening ports,
  firewall state and logged-in users
- **Files** - SFTP browse, upload, download (files and folders), rename, move,
  server-side copy, delete; symlinks listed but never followed
- **Tunnels** - local (`-L`) and remote (`-R`) port forwarding
- **Power** - shutdown, reboot, scheduled or immediate, with cancel; sudo and
  UAC handled per platform

### Containers and hosts without systemd
- Detects Docker, Podman, LXC, Kubernetes and systemd-nspawn containers and
  the host's init system at connect
- Explains, instead of failing, when a container has no service manager or
  cannot reboot itself

### Transfers
- One queue across every host, 1-8 at once, with High/Normal/Low priority
- Downloads land as `.part` and are renamed only when complete
- Overwrite / keep both / skip on conflicts, with apply-to-all

### Keys and local audit
- Browse, generate and delete keys in `~/.ssh`, with permission repair
- Scored audit of your SSH directory

### VPN awareness
- Detects Tailscale, Twingate, NetBird, ZeroTier and WireGuard (read-only)
- Imports tailnet peers as saved hosts
- Explains unreachable VPN addresses, and remembers Twingate's resource list
  while its service is stopped

### Known limitations
- Windows installers are not code-signed; SmartScreen warns on first run
- FIDO security keys work through the SSH agent only
- No automatic updates yet: download new versions from the Releases page

[1.0.0]: https://github.com/PheeLeep/ParolaSSH/releases/tag/v1.0.0
