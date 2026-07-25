# ParolaSSH — roadmap

Where things stand. Updated as work lands.

**Legend** — ✅ done · 🔨 building · 👀 needs your review · 📋 planned · 💭 undecided

---

## Shipped

| Area | State | Notes |
|---|---|---|
| Add / edit / remove connections | ✅ | Atomic JSON store (temp + rename) in the app config dir |
| Groups and tags | ✅ | Free-text group with autocomplete, chip-style tag input |
| Port probe | ✅ | Reads the banner; distinguishes refused / timeout / not-SSH |
| Connect — password, key, agent | ✅ | `russh` + `ring` backend |
| Host key verification | ✅ | Checked during key exchange, before any password is sent |
| Power — shutdown / reboot / cancel | ✅ | Linux, macOS, BSD, Windows; scheduled or immediate |
| sudo vs UAC elevation | ✅ | `sudo -S` over stdin; UAC explained rather than silently failing |
| sudo password reuse | ✅ | Session holds the login password; overridable per action |
| 30-second heartbeat | ✅ | Channel round trip when connected, TCP probe otherwise |
| Interactive terminal (single) | ✅ | PTY, streamed, resizable, UTF-8 reassembly |
| Single instance | ✅ | Second launch focuses the existing window |

### Security fixes applied

| Fix | Why |
|---|---|
| Removed `run_command(String)` | Unused arbitrary-remote-exec IPC verb |
| Terminal events addressed, not broadcast | Output can contain anything you `cat` |
| Output batched (~16 ms / 64 KB) | `yes` would otherwise wedge the UI |
| Passwords `Zeroizing` at the IPC boundary | Plaintext wiped on drop |
| Shell ids on every event | A stale shell's output can't render in a newer pane |

---

## Per-connection feature nav

Dokploy-style horizontal nav inside the host view. Left sidebar picks the
*machine*; the nav picks what you *do* with it.

| Tab | State | Notes |
|---|---|---|
| Overview | 👀 | OS, elevation, host key, heartbeat — all from `connect_host`; icons landed |
| Terminal | 👀 | Multi-shell tabs, cap 8 per host |
| Services | 📋 | Pane explains the plan and the exact commands |
| Performance | 📋 | Same |
| Updates | 📋 | Same |
| Audit | 📋 | Same; tiers below |
| Files | 💭 | SFTP via `russh-sftp`; a real UI on its own |

Unbuilt tabs are reachable and state what they will run, per OS. An empty pane
saying only "coming soon" is worse than no tab — you cannot tell a missing
feature from one that found nothing.

### Terminal — leak prevention ✅

The rule: **the pane stopped owning the shell.** `terminalStore.ts` owns them
and disposes on exactly four events — tab closed, host disconnected, heartbeat
reaped a dead session, app exit. Switching tabs, switching hosts and navigating
away close nothing.

| Leak source | Fix | State |
|---|---|---|
| Tauri `listen()` handles | Held per shell in the store, released in `close()` | ✅ |
| `Terminal.dispose()` | Called in `close()` and on a failed open. Skipping it leaks the renderer's WebGL context — browsers cap those near 16, so this bites before RAM does | ✅ |
| `ResizeObserver` | Created in `attach()`, disconnected by the returned detach | ✅ |
| Rust `HashMap<u64, ShellHandle>` | `remove_shell` on close, `drain_shells` on disconnect and on session replacement | ✅ |
| Scrollback growth | 5000 lines ≈ 1.6 MB per shell; capped at 8 shells per host | ✅ |

Persistence: each terminal's DOM node is created with `document.createElement`,
lives outside React's tree, and is re-parented into whichever pane is showing.
The `Terminal` object is never destroyed on navigation, so scrollback survives
leaving the page — and output keeps arriving while it is off screen.

Three bugs found and fixed during the leak review, all in `TerminalTabs`:

| Bug | Symptom |
|---|---|
| Adopt effect depended on `openTerminal` | Flipping the theme reopened a terminal the user had closed |
| Attach effect depended on `terminals.length` | Opening a second tab tore down and rebuilt the first one's `ResizeObserver` |
| Empty state rendered inside the mount node | React managing children of an element the store also appends to |

---

## Decisions taken

**Feature nav is horizontal, not a second sidebar.** Two 264 px rails leave
372 px for the terminal at the 900 px window minimum — about 43 columns, under
the 80 that `htop` and `systemctl status` assume.

**One connection, many channels.** Extra shells, metrics polling and service
queries all share the single authenticated session. No second handshake, no
second password — the same thing OpenSSH does with `ControlMaster`.

**Windows gets what Windows has, not a journald imitation.** Verified: on
Linux, `journalctl -u X` works unprivileged when the user is in `adm`. Windows
has no per-service log — provider names rarely match service names, and most
services log to their own files. So the third column differs by platform:

| | Linux | Windows |
|---|---|---|
| List | `systemctl list-units` | `sc query` (native, no PowerShell startup) |
| Actions | start / stop / restart | start / stop / restart |
| History | **Logs** — `journalctl -u X`, follow supported | **Recent events** — SCM 7036 / 7031 / 7034 via `wevtutil` |

**Icons — done.** ✅ Checked the installed `lucide-react` (5,987 icons): no
`Linux`, `Windows`, `Ubuntu` or `Debian`. `Apple` exists but is *the fruit*.
Brand icons were dropped deliberately and aren't returning.

`OsIcon.tsx` inlines Linux, Apple and FreeBSD marks from Simple Icons (CC0),
**generated from the package rather than transcribed**, so the geometry is
exact. `simple-icons` was installed with `--no-save`, read, and removed — no
runtime or build dependency, ~7 KB of paths.

**Windows has no logo, on purpose.** Simple Icons carries no Windows mark;
Microsoft had theirs removed. Hand-recreating it would mean reproducing a
trademark whose owner objected to exactly that, so Windows gets Lucide's
neutral `AppWindow` glyph instead.

Marks render in `currentColor`, not brand colours — at 16 px on a tinted badge,
Ubuntu orange and Apple black fight the surface and read as decoration.

Overview tiles carry Lucide glyphs beside the *label*, not the value, so a row
of tiles scans as a column of numbers: `UserRound`, `Clock`, `Activity`,
`Signal`, `Network`, `History`, `Fingerprint`. The feature nav uses `Server`,
`SquareTerminal`, `Boxes`, `Gauge`, `Package`, `ShieldCheck`, `FolderOpen`.

**Audit is tiered — Lynis is not the starting point.**

| Tier | What | Cost |
|---|---|---|
| 0 | Crypto from the handshake russh already did — weak kex, CBC ciphers, `ssh-rsa`+SHA-1 | Free; no remote command, no privileges |
| 1 | `sshd -T` posture, `authorized_keys` perms, empty passwords, world-writable PATH | A handful of read-only commands |
| 2 | Lynis, **opt-in only** — detect `command -v lynis`, never install | Minutes, pegs a core |

Verified: Lynis is not installed on the test VM, which is the normal case.
Silently installing a package on someone's server from a GUI is out.

---

## Open questions

| Question | State |
|---|---|
| Status dot — "reachable" (current) or "we have a session"? | 💭 Your call |
| Per-host shell cap of 8 — right number? | 👀 Shipped at 8; easy to change |
| Does Services ship Linux-first, or both platforms together? | 💭 Asked, unanswered |
| Should tab titles be renameable? `rename()` exists, no UI yet | 💭 |
| Two rows of tabs (feature nav + shell tabs) — acceptable? | 👀 Built; judge it live |

---

## Testing

| Suite | Command | Count |
|---|---|---|
| Rust unit | `cargo test --lib` | 69 |
| Rust live (needs the VM) | see below | 7 |
| Frontend | `npx tsc --noEmit` | typecheck only |

```sh
PAROLASSH_LIVE_HOST=192.168.56.10 \
PAROLASSH_LIVE_USER=pheeleep \
PAROLASSH_LIVE_PASSWORD=… \
cargo test --test live_remote -- --test-threads=1
```

The power test schedules a reboot 600 minutes out and cancels it, then asserts
`/run/systemd/shutdown/scheduled` is gone. Nothing in the suite reboots
anything.
