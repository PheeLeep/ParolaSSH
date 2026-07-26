<p align="center">
  <img src="design/icon.svg" width="120" alt="ParolaSSH">
</p>

<h1 align="center">ParolaSSH</h1>

<p align="center">
  <b>ParolaSSH</b> (<i>parola</i> — "lighthouse" in Filipino) is a GUI SSH remote console
  and server manager: keep your machines in one place, watch them, and administer
  them without remembering every flag and log path.<br>
  Built with <b>Tauri 2</b>, <b>React 19</b> and <b>Rust</b> (<code>russh</code>).
</p>

<p align="center">
  <img alt="Linux" src="https://img.shields.io/badge/Linux-tested-6b46c1">
  <img alt="Windows" src="https://img.shields.io/badge/Windows-tested-6b46c1">
  <img alt="macOS" src="https://img.shields.io/badge/macOS-untested-8a8598">
  <img alt="MIT" src="https://img.shields.io/badge/license-MIT-6b46c1">
</p>

---

<!-- Screenshots go here -->

Routine work — services, updates, load, power — lives in panes, while the real
terminal stays one click away. It **reports and acts on your command**: it never
installs packages, never changes a server's configuration on its own, and never
sends a credential you did not choose.

## Features

**Connections**
- Saved hosts with groups, tags and free-text search
- Auth via password, private key, SSH agent, or `none` (Tailscale SSH)
- Host key verification during key exchange — before any password is sent
- Port probe that distinguishes refused / timeout / not-SSH
- 30-second heartbeat with four honest status states

**Per-host panes**
| Pane | What it does |
|---|---|
| Overview | OS, elevation, host key, uptime, heartbeat |
| Terminal | Real PTY, multi-shell tabs (max 8), renameable, persistent scrollback |
| Services | `systemctl` / `sc query` — start, stop, restart, plus journal or SCM events with live follow |
| Performance | CPU, memory, load, disks; user-set 1–30 s sampling |
| Updates | Pending apt/dnf packages; Windows hotfix history |
| Audit | Handshake crypto, `sshd -T` posture, key permissions — with per-host dismissals |

**Keys & audit**
- Browse, generate and delete keys in `~/.ssh`, with permission repair
- Scored audit of your local SSH directory (weak algorithms, exposed keys, loose modes)

**VPN awareness**
- Detects Tailscale, Twingate, NetBird, ZeroTier and WireGuard — read-only, no login, no `up`
- Imports tailnet peers as saved hosts, MagicDNS address preferred
- Explains a CGNAT address that can't be reached because the client is down

**Elsewhere**
- One session, many channels — extra shells and polling share a single handshake
- Sessions view across every host, sudo/UAC elevation handled per platform
- Light / dark / system themes, motion controls, single-instance launch

## Build it yourself

Prerequisites: **Node 18+**, **Rust (stable)**, and the
[Tauri 2 system dependencies](https://tauri.app/start/prerequisites/) for your
platform — WebKitGTK + `libssl-dev` on Linux, WebView2 on Windows.

```sh
git clone https://github.com/PheeLeep/ParolaSSH.git
cd ParolaSSH
npm install

npm run tauri dev      # run it
npm run tauri build    # bundle for your OS → src-tauri/target/release/bundle/
```

Linux desktop entry for dev builds: `scripts/install-dev-desktop-entry.sh`.

## Tests

```sh
cd src-tauri
cargo test --lib                       # unit
cargo test --test audit_fixtures       # parser fixtures
npx tsc --noEmit                       # frontend typecheck (from repo root)
```

No test runs a real CLI or touches a real machine. The live suite is `#[ignore]`d
and needs a VM you name explicitly:

```sh
PAROLASSH_LIVE_HOST=… PAROLASSH_LIVE_USER=… PAROLASSH_LIVE_PASSWORD=… \
  cargo test --test live_remote -- --ignored --test-threads=1
```

## Where things live

```
src/features/…      hosts, keys, sessions, vpn, settings (React)
src-tauri/src/ssh/    local key store, audit, known_hosts
src-tauri/src/remote/ sessions, shells, services, metrics, updates
src-tauri/src/vpn/    per-provider status detection
docs/ROADMAP.md       what's shipped, what's decided, and why
```

Hosts and settings live in the app config directory, written owner-only.

## Platform support

Linux and Windows are developed and tested against real VMs. **macOS and BSD are
written for but untested** — the power, metrics and audit paths have macOS/BSD
branches, and no machine has ever run them. Treat it as unsupported until someone
does; reports welcome.

## License

[MIT](LICENSE) © PheeLeep

---

Status: **early** — v0.1.0. See [ROADMAP.md](docs/ROADMAP.md) for the current state.
