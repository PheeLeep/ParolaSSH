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
| Sessions view | ✅ | Every open shell across hosts, live count in the sidebar, links straight to the tab |
| Single instance | ✅ | Second launch focuses the existing window |

### Security fixes applied

| Fix | Why |
|---|---|
| Removed `run_command(String)` | Unused arbitrary-remote-exec IPC verb |
| Terminal events addressed, not broadcast | Output can contain anything you `cat` |
| Output batched (~16 ms / 64 KB) | `yes` would otherwise wedge the UI |
| Passwords `Zeroizing` at the IPC boundary | Plaintext wiped on drop |
| Shell ids on every event | A stale shell's output can't render in a newer pane |
| Followed logs inherit all three rules | Addressed events, batching, stream ids — a journal contains whatever the machine logs |
| No generic stream-open verb | Each feature opens its own typed command; only `close_stream(id)` is generic |

---

## Per-connection feature nav

Dokploy-style horizontal nav inside the host view. Left sidebar picks the
*machine*; the nav picks what you *do* with it.

| Tab | State | Notes |
|---|---|---|
| Overview | ✅ | OS, elevation, host key, heartbeat — all from `connect_host`; reviewed |
| Terminal | ✅ | Multi-shell tabs, cap 8 per host; reviewed, cap stays at 8 |
| Services | ✅ | List, start/stop/restart with the sudo/UAC route, journal + follow / SCM events |
| Performance | ✅ | CPU, memory, load, uptime, disks; pane-scoped sampling, user-set 1–30 s |
| Updates | ✅ | apt/dnf pending list; Windows shows hotfix history when PSWindowsUpdate is absent |
| Audit | ✅ | Tiers 0–1 built, tier 2 is detect-only; details below |
| Files | 💭 | SFTP via `russh-sftp`; a real UI on its own |

The Files tab still states what it will run rather than showing an empty
"coming soon" pane — you cannot tell a missing feature from one that found
nothing.

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

| Tier | What | State |
|---|---|---|
| 0 | Crypto from the handshake russh already did — weak kex, CBC ciphers, `ssh-rsa`+SHA-1, strict-kex | ✅ via `Handler::kex_done`; no remote command, no privileges |
| 1 | `sshd -T` posture, `authorized_keys` perms, empty passwords, world-writable PATH | ✅ read-only; degrades honestly without root |
| 2 | Lynis, **opt-in only** — detect `command -v lynis`, never install | Detection shipped; the run itself is deferred |

Verified: Lynis is not installed on the test VM, which is the normal case.
Silently installing a package on someone's server from a GUI is out.

Tier-0 honesty: russh's default preference lists contain no SHA-1 kex and no
CBC cipher, so those two rules can never fire against the current client — a
server offering only weak algorithms fails to connect instead. They are kept
as guards in case the lists are ever widened for legacy devices; the checks
that bite today are the `ssh-rsa` host key and missing strict kex.

Remote findings score with the same weights and diminishing returns as the
local key audit, and dismissals persist per host
(`remote-audit-suppressions.json`, keyed `host|rule|target`) — silencing a
finding on one box never hides it on another.

**Streaming exec exists now, separately from the terminal.** `Session::exec`
keeps its deliberate 30-second ceiling; `journalctl -f` runs on a new
`stream` path — a PTY-less copy of the shell plumbing with the same 16 ms /
64 KB batching, the same UTF-8 reassembly, and the same addressed
(`emit_to`) events, because a followed log contains whatever the machine
writes to it. Streams live in the registry beside shells (cap 4 per host)
and are drained at the same four moments, so a followed journal cannot
outlive its session. The one slow-but-bounded exception is
`Get-WindowsUpdate`, which gets `exec_with_timeout(120 s)`.

**Performance samples on a pane-scoped timer, not the heartbeat.** The
earlier plan said "sample on the heartbeat", but 30 s is uselessly coarse
for watching a load spike, and sampling every saved host forever is the
wrong default. The cadence is the user's to pick (1/2/5/10/30 s, default
1 s), and the pane polls only while mounted and visible; one compound exec
per sample. CPU% is the delta between successive `/proc/stat` readings,
with the previous reading held on the Rust session — the first sample
honestly shows "—" rather than sleeping a second inside the command. The
figure is the whole-machine aggregate across all cores (a process pegging
one core of four reads as 25%), shown as a whole number.

**Windows updates are reported honestly.** Querying *pending* updates from
the CLI needs the PSWindowsUpdate module, which most machines lack. Absent
module, the pane says so and lists recent installed hotfixes instead — and
never installs the module to improve its own answer. Nothing on any
platform is ever installed from the Updates pane; it reports, the operator
decides in a terminal.

---

## Open questions

| Question | State |
|---|---|
| Status dot semantics | ✅ Decided: four states. Green + halo = live session, amber = reachable but no session, red = unreachable, grey = never probed. Fixing this also fixed a real bug: the hosts table treated "reachable" as "connected" and showed a Disconnect button for hosts we held no session on. `onlineCount` became `connectedCount` and counts sessions only. |
| Per-host shell cap of 8 — right number? | ✅ Reviewed; stays at 8 |
| Does Services ship Linux-first, or both platforms together? | ✅ Both shipped together (cross-platform parity is the rule) |
| Should tab titles be renameable? `rename()` exists, no UI yet | 💭 |
| Two rows of tabs (feature nav + shell tabs) — acceptable? | ✅ Reviewed; kept |

---

## Testing

| Suite | Command | Count |
|---|---|---|
| Rust unit | `cargo test --lib` | 142 |
| Rust fixtures | `cargo test --test audit_fixtures` | 36 |
| Rust live (needs the VM) | see below | 7 |
| Frontend | `npx tsc --noEmit` | typecheck only |

The new feature modules follow the `power.rs` testing shape: command
construction is pure and asserted as exact strings per OS (including the
quoting/injection cases), and parsers are fed captured-or-invented fixture
literals — `systemctl --plain`, `sc query` CRLF blocks, `wevtutil /f:text`,
`/proc` files, apt/dnf output (dnf's exit 100 included), CIM JSON, `sshd -T`.
No test runs a real CLI, and no fixture contains real tenant data.

```sh
PAROLASSH_LIVE_HOST=192.168.56.10 \
PAROLASSH_LIVE_USER=pheeleep \
PAROLASSH_LIVE_PASSWORD=… \
cargo test --test live_remote -- --test-threads=1
```

The power test schedules a reboot 600 minutes out and cancels it, then asserts
`/run/systemd/shutdown/scheduled` is gone. Nothing in the suite reboots
anything.
