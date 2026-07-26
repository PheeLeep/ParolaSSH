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
| sudo password reuse | ✅ | Session holds the login password; overridable per action. Taken from the *resolved* credential, so a reconnect that recalled its password from the vault can still elevate |
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
| Windows service names refuse `$(`, `${`, `` ` `` | `"…"` quotes identically in cmd.exe and PowerShell; only interpolation differs. Refusing the substitution openers keeps one quoting scheme correct against either shell, with no probe and no registry read. A bare `$` stays legal — `MSSQL$SQLEXPRESS` is a real service |
| `hosts.json` written owner-only | It is a list of every machine you administer — the same "readable map" the audit scores an unhashed `known_hosts` for. Was `0644` under the default umask |

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
| Files | ✅ | SFTP browse, upload, download, delete; symlinks listed but never followed |

### Files — the two rules ✅

Symlinks and device files are listed with their target and refused for every
operation. Following one is how a host makes a download read `/dev/zero`
forever or land outside the folder you picked, and resolving safely means a
containment check racing the server's own filesystem. `refuse_unless_regular`
is the single gate, shared by descend, download and delete.

SFTP has no sudo — the subsystem runs as the login user and there is no
elevation to offer. A denied path says so and names the fix (reconnect as a
user with access) rather than showing a prompt that cannot work. Note that the
denial only surfaces on *open*: `stat` needs no read permission, so
`/etc/shadow` stats fine and fails when its bytes are asked for.

### Transfers — one queue for every host ✅

Rationed globally rather than per host, because what fills up is the local
uplink: five downloads from five hosts saturate a connection exactly as five
from one would. Default three at once, settable 1–8 in Settings. Priority is
High/Normal/Low, ties broken by arrival, with each waiting row showing its
position. Lowering the cap never interrupts a transfer already running.

Downloads stage as `{name}.part` and are renamed only after the last byte, so a
cancel or a dropped link never leaves a truncated file wearing the real name,
and they are created `0600` on Unix so a fetched private key is never
world-readable. Nothing resumes across a restart; a host that disconnects fails
its transfers visibly instead of letting them vanish.

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

## Tailscale peer import ✅

Import tailnet machines as saved hosts, from the Tailscale tab of the VPN page.

**No local API socket was needed.** `tailscale status --json` already carries
the `Peer` map, so this parses more of the payload the status check was
fetching anyway — no new dependency, no new platform surface, and `run_cli`
stays the whole cross-platform story. The earlier worry in `tailscale.rs` (HTTP
over a Unix socket on two platforms, a named pipe on the third) does not apply.

| Decision | Why |
|---|---|
| `PeerListing` is an enum, not a `Vec` | A logged-out client returns an empty peer map. Rendering that as "no machines" would be a lie; it has to say *"Tailscale needs login"* |
| MagicDNS address preferred | It survives the node changing address; the 100.x one does not. Falls back to the IP, then the bare hostname |
| IPv4 picked from `TailscaleIPs` | The list mixes v4 and v6; 100.64/10 is the one an operator recognises |
| `tag:server` → `server` | Drops straight into the existing tag input |
| Online first, then alphabetical | The reachable machines are the ones you are there to add |
| One username + auth method for the batch | Tailscale knows the address, never the account. Tailnets are usually one login; anything unusual is a normal edit afterwards |
| Default auth is `agent`, **not** `none` | Tailscale SSH's server is Linux-only *and* opt-in, so `none` is wrong more often than right for a batch. Choosing it names the selected peers that cannot serve it, using the `OS` field. `status --json` carries **no** per-peer SSH capability — checked against a real tailnet, the peer fields are `Active, Addrs, AllowedIPs, Created, CurAddr, DNSName, ExitNode, …` with nothing SSH-shaped — so per-node detection is not possible from this source |
| Already-saved peers shown but not selectable | Matched on every address a peer answers to, so a second import cannot duplicate |
| Saved sequentially | Each save rewrites `hosts.json`; concurrent writes would race for the file |

Still read-only toward Tailscale: no login, no `tailscale up`, no daemon start.
All nine parser tests are mock-driven — no Tailscale installed, no real CLI.

### Auth method `none` ✅

Added because peer import shipped without it and was therefore incomplete:
**Tailscale SSH nodes could not be connected to at all.** Tailscale
authenticates the node over WireGuard before the SSH layer is reached, then
offers SSH's `none` method — its own docs note that *"Some SSH clients may fail
to connect to an SSH server using no authentication."* ParolaSSH was one of
them, since `AuthMethod` had only password/publickey/agent.

`russh` already had `authenticate_none`, so the change is an enum variant
threaded through `build_credentials` and `authenticate`. Two rules:

- **Never a fallback.** It is chosen per host, so "no credential was sent" is
  always something the operator asked for, never something the app decided.
- **A remembered password is still not sent** — asserted by a test that puts one
  in the vault and checks it stays there.

The form and the import dialog both say plainly that an ordinary sshd will
refuse this, and the failure message names Tailscale SSH as the case where it
does work.

Verified against the Windows VM over its **tailnet** address: it answers with
Windows OpenSSH 9.5 offering `publickey,password,keyboard-interactive` — the
same host key as its LAN address, and no `none`. Per Tailscale's docs the SSH
server component is *"only available on: Linux, macOS open source"*, so a
Windows peer can never accept `none` however the tailnet routes to it. Reaching
a node over Tailscale and being served **by** Tailscale are different things.

## Open questions

| Question | State |
|---|---|
| Status dot semantics | ✅ Decided: four states. Green + halo = live session, amber = reachable but no session, red = unreachable, grey = never probed. Fixing this also fixed a real bug: the hosts table treated "reachable" as "connected" and showed a Disconnect button for hosts we held no session on. `onlineCount` became `connectedCount` and counts sessions only. |
| Per-host shell cap of 8 — right number? | ✅ Reviewed; stays at 8 |
| Does Services ship Linux-first, or both platforms together? | ✅ Both shipped together (cross-platform parity is the rule) |
| Should tab titles be renameable? `rename()` exists, no UI yet | ✅ Shipped. Double-click the tab, or the pencil beside the font and clear buttons; F2 works while the tab itself holds focus. Enter and blur commit, Escape reverts, and an empty name restores the `shell N` it opened with rather than leaving the tab holding a name you just tried to delete. Titles live in the store with everything else about a shell, so they die with the app — the same lifetime as the scrollback beside them |
| Two rows of tabs (feature nav + shell tabs) — acceptable? | ✅ Reviewed; kept |

---

## Testing

| Suite | Command | Count |
|---|---|---|
| Rust unit | `cargo test --lib` | 158 |
| Rust fixtures | `cargo test --test audit_fixtures` | 40 |
| Rust live (needs a VM) | see below | 11, all `#[ignore]`d · green on Linux **and** Windows |
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
cargo test --test live_remote -- --ignored --nocapture --test-threads=1
```

The live tests are `#[ignore]`d, so a default run reports them *ignored* rather
than passed. They previously returned early when the environment was unset,
which libtest counted as **11 passed** against a machine they had never
contacted. Asking for them without naming a host is now a hard failure.

All 11 assert on both Linux and Windows — verified green against a Windows 11
VM (`OpenSSH_for_Windows_9.5`). What used to skip now asserts the *other*
platform's behaviour: Windows CPU needs no delta (`LoadPercentage` is
instantaneous), tier 1 is Unix-only so the report must assemble from tier 0
alone with `tier1_ran = false`, Windows power needs no password and no `sudo`,
and the reboot cancel is verified by a second `shutdown /a` failing with 1116.

`skip()` remains only for macOS/BSD, which no VM covers yet. A skip still
reports `ok` — libtest has no runtime "skipped" outcome — so prefer a per-OS
assertion over calling it.

The power test schedules a reboot 600 minutes out and cancels it, then asserts
`/run/systemd/shutdown/scheduled` is gone. Nothing in the suite reboots
anything.
