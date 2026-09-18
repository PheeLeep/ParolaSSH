# Changelog

All notable changes to ParolaSSH. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- A PKGBUILD in `packaging/arch` for Arch-based systems, running on the
  system WebKitGTK instead of the AppImage's bundled copy

## [1.0.3] - 2026-09-18

### Fixed
- Choppy scrolling in the Linux AppImage: it now runs on Wayland in a
  Wayland session instead of the slower XWayland it was forced onto. Set
  `PAROLASSH_FORCE_X11=1` to keep X11

### Added
- A debug-level startup log line with the WebKitGTK version and display
  settings, for rendering bug reports

## [1.0.2] - 2026-09-18

### Fixed
- Choppy scrolling on Linux (seen on Wayland with hybrid graphics): the
  webview now draws on the CPU, unless `WEBKIT_SKIA_ENABLE_CPU_RENDERING` is
  already set in the environment

## [1.0.1] - 2026-09-18

### Fixed
- Every page uses the same wide layout, so none sits centred in an empty
  gutter on a wide window

## [1.0.0] - 2026-09-18

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
- **Tasks** - built-in and your own one-click commands in one list. Output
  opens in a dialog; closing it keeps the task running, and each task keeps
  its last result to reopen. A danger check comes before anything destructive. Settings › Advanced › **Block dangerous tasks**
  (on by default) refuses destructive tasks outright, or everything the check
  flags; turned off, a typed confirmation is asked instead
- **Security** - handshake crypto, `sshd -T` posture, listening ports,
  firewall state and logged-in users. On Windows the posture audit also
  checks who can write sshd_config and the authorized_keys files, who can read
  the host keys, and writable PATH folders; plus a read-only Microsoft
  Defender view (protection switches, definition age, scans, detections, and
  any third-party antivirus in charge)
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

### Updates
- Checks GitHub for a newer release at launch (can be turned off) and offers it
  in a banner; nothing installs without a click
- Windows installers and the AppImage update in place, after the download is
  verified against the app's signing key; a warning comes first if sessions
  are open

### Known limitations
- Windows installers are not code-signed; SmartScreen warns on first run
- FIDO security keys work through the SSH agent only
- `.deb` and `.rpm` installs are told about updates but download them by hand

[1.0.3]: https://github.com/PheeLeep/ParolaSSH/releases/tag/v1.0.3
[1.0.2]: https://github.com/PheeLeep/ParolaSSH/releases/tag/v1.0.2
[1.0.1]: https://github.com/PheeLeep/ParolaSSH/releases/tag/v1.0.1
[1.0.0]: https://github.com/PheeLeep/ParolaSSH/releases/tag/v1.0.0
