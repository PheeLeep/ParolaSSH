# ParolaSSH - roadmap

Where things stand. Updated as work lands.

**Legend** - ✅ done · 🔨 building · 👀 needs your review · 📋 planned · 💭 undecided

---

## Shipped

| Area | State | Notes |
|---|---|---|
| Add / edit / remove connections | ✅ | Atomic JSON store (temp + rename) in the app config dir |
| Groups and tags | ✅ | Free-text group with autocomplete, chip-style tag input |
| Port probe | ✅ | Reads the banner; distinguishes refused / timeout / not-SSH |
| Connect - password, key, agent | ✅ | `russh` + `ring` backend |
| Host key verification | ✅ | Checked during key exchange, before any password is sent |
| Power - shutdown / reboot / cancel | ✅ | Linux, macOS, BSD, Windows; scheduled or immediate |
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
| Followed logs inherit all three rules | Addressed events, batching, stream ids - a journal contains whatever the machine logs |
| No generic stream-open verb | Each feature opens its own typed command; only `close_stream(id)` is generic |
| Windows service names refuse `$(`, `${`, `` ` `` | `"…"` quotes identically in cmd.exe and PowerShell; only interpolation differs. Refusing the substitution openers keeps one quoting scheme correct against either shell, with no probe and no registry read. A bare `$` stays legal - `MSSQL$SQLEXPRESS` is a real service |
| `hosts.json` written owner-only | It is a list of every machine you administer - the same "readable map" the audit scores an unhashed `known_hosts` for. Was `0644` under the default umask |

---

## Per-connection feature nav

Dokploy-style horizontal nav inside the host view. Left sidebar picks the
*machine*; the nav picks what you *do* with it.

| Tab | State | Notes |
|---|---|---|
| Overview | ✅ | OS, elevation, host key, heartbeat - all from `connect_host`; reviewed |
| Terminal | ✅ | Multi-shell tabs, cap 8 per host; reviewed, cap stays at 8 |
| Services | ✅ | List, start/stop/restart with the sudo/UAC route, journal + follow / SCM events |
| Performance | ✅ | CPU, memory, load, uptime, disks; pane-scoped sampling, user-set 1–30 s |
| Updates | ✅ | apt/dnf pending list; Windows shows hotfix history when PSWindowsUpdate is absent |
| Audit | ✅ | Tiers 0–2 built, tier 2 opt-in behind its own consent; details below |
| Files | ✅ | SFTP browse, transfer, rename/move/copy, delete; symlinks listed but never followed |

### Files - the two rules ✅

Symlinks and device files are listed with their target and refused for every
operation. Following one is how a host makes a download read `/dev/zero`
forever or land outside the folder you picked, and resolving safely means a
containment check racing the server's own filesystem. `refuse_unless_regular`
is the single gate, shared by descend, download and delete.

SFTP has no sudo - the subsystem runs as the login user and there is no
elevation to offer. A denied path says so and names the fix (reconnect as a
user with access) rather than showing a prompt that cannot work. Note that the
denial only surfaces on *open*: `stat` needs no read permission, so
`/etc/shadow` stats fine and fails when its bytes are asked for.

### Files - operations ✅

Rename and move are the same SFTP request, and it is deliberately allowed to
fail: the protocol's rename refuses an existing destination, so "move" can never
silently destroy a file the user did not name. Copy has no SFTP equivalent, so it
runs `cp -a` (or `Copy-Item -Recurse`) on the server - the one file operation
needing a shell, quoted with the same helper as the power and service commands,
and the only one that keeps a 10 GB duplicate off the wire entirely.

Folder download walks the tree first and queues one transfer per regular file,
so progress, priority and cancellation stay per-file. Symlinks and device files
are skipped and counted; empty directories are not recreated, since only files
are transferred.

A destination that is already taken prompts for overwrite / keep both / skip,
with an apply-to-all for recursive transfers. This was not always so: uploads
opened with `CREATE|TRUNCATE` and silently replaced whatever was there.

### Transfers - one queue for every host ✅

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

### Terminal - leak prevention ✅

The rule: **the pane stopped owning the shell.** `terminalStore.ts` owns them
and disposes on exactly four events - tab closed, host disconnected, heartbeat
reaped a dead session, app exit. Switching tabs, switching hosts and navigating
away close nothing.

| Leak source | Fix | State |
|---|---|---|
| Tauri `listen()` handles | Held per shell in the store, released in `close()` | ✅ |
| `Terminal.dispose()` | Called in `close()` and on a failed open. Skipping it leaks the renderer's WebGL context - browsers cap those near 16, so this bites before RAM does | ✅ |
| `ResizeObserver` | Created in `attach()`, disconnected by the returned detach | ✅ |
| Rust `HashMap<u64, ShellHandle>` | `remove_shell` on close, `drain_shells` on disconnect and on session replacement | ✅ |
| Scrollback growth | 5000 lines ≈ 1.6 MB per shell; capped at 8 shells per host | ✅ |

Persistence: each terminal's DOM node is created with `document.createElement`,
lives outside React's tree, and is re-parented into whichever pane is showing.
The `Terminal` object is never destroyed on navigation, so scrollback survives
leaving the page - and output keeps arriving while it is off screen.

Three bugs found and fixed during the leak review, all in `TerminalTabs`:

| Bug | Symptom |
|---|---|
| Adopt effect depended on `openTerminal` | Flipping the theme reopened a terminal the user had closed |
| Attach effect depended on `terminals.length` | Opening a second tab tore down and rebuilt the first one's `ResizeObserver` |
| Empty state rendered inside the mount node | React managing children of an element the store also appends to |

---

## Decisions taken

**Feature nav is horizontal, not a second sidebar.** Two 264 px rails leave
372 px for the terminal at the 900 px window minimum - about 43 columns, under
the 80 that `htop` and `systemctl status` assume.

**One connection, many channels.** Extra shells, metrics polling and service
queries all share the single authenticated session. No second handshake, no
second password - the same thing OpenSSH does with `ControlMaster`.

**Windows gets what Windows has, not a journald imitation.** Verified: on
Linux, `journalctl -u X` works unprivileged when the user is in `adm`. Windows
has no per-service log - provider names rarely match service names, and most
services log to their own files. So the third column differs by platform:

| | Linux | Windows |
|---|---|---|
| List | `systemctl list-units` | `sc query` (native, no PowerShell startup) |
| Actions | start / stop / restart | start / stop / restart |
| History | **Logs** - `journalctl -u X`, follow supported | **Recent events** - SCM 7036 / 7031 / 7034 via `wevtutil` |

**Icons - done.** ✅ Checked the installed `lucide-react` (5,987 icons): no
`Linux`, `Windows`, `Ubuntu` or `Debian`. `Apple` exists but is *the fruit*.
Brand icons were dropped deliberately and aren't returning.

`OsIcon.tsx` inlines Linux, Apple and FreeBSD marks from Simple Icons (CC0),
**generated from the package rather than transcribed**, so the geometry is
exact. `simple-icons` was installed with `--no-save`, read, and removed - no
runtime or build dependency, ~7 KB of paths.

**Windows has no logo, on purpose.** Simple Icons carries no Windows mark;
Microsoft had theirs removed. Hand-recreating it would mean reproducing a
trademark whose owner objected to exactly that, so Windows gets Lucide's
neutral `AppWindow` glyph instead.

Marks render in `currentColor`, not brand colours - at 16 px on a tinted badge,
Ubuntu orange and Apple black fight the surface and read as decoration.

Overview tiles carry Lucide glyphs beside the *label*, not the value, so a row
of tiles scans as a column of numbers: `UserRound`, `Clock`, `Activity`,
`Signal`, `Network`, `History`, `Fingerprint`. The feature nav uses `Server`,
`SquareTerminal`, `Boxes`, `Gauge`, `Package`, `ShieldCheck`, `FolderOpen`.

**Audit is tiered, and every tier is the app's own work.**

| Tier | What | State |
|---|---|---|
| 0 | Crypto from the handshake russh already did - weak kex, CBC ciphers, `ssh-rsa`+SHA-1, strict-kex | ✅ via `Handler::kex_done`; no remote command, no privileges |
| 1 | `sshd -T` posture, `authorized_keys` perms, empty passwords, world-writable PATH | ✅ read-only; degrades honestly without root |

A third-party deep scanner (Lynis) was built and then removed: driving another
tool's minutes-long run was its own subsystem, and the ground it covered belongs
to the Tasks module instead - which keeps the rule that shaped the first
attempt, asserted by a test rather than written in a comment: nothing is ever
installed on someone's server from a GUI.

Tier-0 honesty: russh's default preference lists contain no SHA-1 kex and no
CBC cipher, so those two rules can never fire against the current client - a
server offering only weak algorithms fails to connect instead. They are kept
as guards in case the lists are ever widened for legacy devices; the checks
that bite today are the `ssh-rsa` host key and missing strict kex.

Remote findings score with the same weights and diminishing returns as the
local key audit, and dismissals persist per host
(`remote-audit-suppressions.json`, keyed `host|rule|target`) - silencing a
finding on one box never hides it on another.

**Streaming exec exists now, separately from the terminal.** `Session::exec`
keeps its deliberate 30-second ceiling; `journalctl -f` runs on a new
`stream` path - a PTY-less copy of the shell plumbing with the same 16 ms /
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
with the previous reading held on the Rust session - the first sample
honestly shows "-" rather than sleeping a second inside the command. The
figure is the whole-machine aggregate across all cores (a process pegging
one core of four reads as 25%), shown as a whole number.

**Windows updates are reported honestly.** Querying *pending* updates from
the CLI needs the PSWindowsUpdate module, which most machines lack. Absent
module, the pane says so and lists recent installed hotfixes instead - and
never installs the module to improve its own answer. Nothing on any
platform is ever installed from the Updates pane; it reports, the operator
decides in a terminal.

---

## Tasks - one-click commands ✅

A task is a command with a name. Two kinds share one pane and are never
interleaved, because the app is making a different claim about each.

| | Built in | Yours |
|---|---|---|
| Authored by | `tasks/catalog.rs`, per OS, in Rust | the operator |
| Command | constructed and unit-tested | run verbatim, never rewritten |
| The claim | this is what it does | this is what you typed |

**Global means "offered everywhere", not "runs everywhere".** A global task
appears on every host whose OS it supports; pressing it runs on the one machine
being looked at. There is no fan-out - one press, one host, which is the only
version of this feature where the confirmation dialog can honestly name what it
is about to touch. Per-host tasks are pinned to one id and are deleted with the
host record (`forget_host_tasks`, called from the same place the record goes),
so the file cannot accumulate commands aimed at machines that no longer exist.

| Decision | Why |
|---|---|
| Plan and run are two commands | `plan_task` builds the exact string and assesses it; `start_task` runs it. The UI must call the first and show what it returns before it may call the second |
| The command is never sent back across the boundary | `start_task` takes a *task id* and rebuilds the plan server-side. A window that showed one command and submitted another would be the worst bug this module could have, and passing the command through the webview is what would make it possible |
| Elevation is per task, and per press | `elevated` is a field the operator sets, not something inferred from what the command looks like. A press can override it. A task that says it runs as root and cannot **errors** - it never quietly runs as someone else |
| The `sudo` wrapper is `sh -c '…'` | So a task with pipes, redirects or several statements elevates as a whole rather than only its first word. Both forms are shown: the wrapper is the app's doing and is labelled as such |
| Runs on the `stream` path | `Session::exec` caps at thirty seconds. A backup that takes four minutes must not be cut off with its output discarded |
| One run per host | Starting a second while one is live **throws** rather than returning quietly - a button that does nothing and says nothing is indistinguishable from a broken one. The same lesson the removed Lynis card paid for |
| Stopping stops *watching* | Closing the channel does not reach in and kill a process running on the host. The feed says exactly that rather than implying the command was cancelled |
| An unidentified OS is offered nothing | `OsFamily::Unknown` gets no built-ins. Picking a family and hoping is how the wrong command runs |

**A stream that ends by itself releases its own slot.** Found by running five
tasks in a row on one host: the fifth was refused with "too many streams open"
while nothing was running. Only `close_stream` ever called `remove_stream`, so
every stream that finished on its own stayed in the registry counting against
`MAX_STREAMS_PER_HOST` - a leak the Tasks pane merely exposed, since it is the
first feature that opens and finishes short streams repeatedly. `batch_and_emit`
now reaps the handle at the moment it emits `stream://closed`, which fixes the
followed-journal path with it.

The run store is `taskStore.ts`, keyed by host and disposed on the same four
moments a terminal is - disconnected, heartbeat reaped it, stopped, app exit.
Switching hosts or tabs closes nothing. The feed is an xterm for the reason the
Lynis feed had to become one: a real command emits cursor and colour escapes,
and a `<pre>` renders them as literal text.

### The danger check ✅

`tasks/danger.rs` reads a command for the shapes that end badly and reports what
it finds. It is worth being precise about what this is:

**It is a typo catcher, not a security boundary.** It matches text. A command
that hides its intent - behind a variable, a base64 blob, a script it downloads
- walks straight past every rule, and adding rules does not change that. The
operator writes these commands and already holds the credentials to run them by
hand. What is being defended against is the stray `/`, the line pasted from a
forum, the task written for the wrong host.

That framing decides the tuning, and the tuning is the whole feature:

| Decision | Why |
|---|---|
| Three levels, and `none` never says "safe" | `none` means *nothing matched*. The dialog shows no green tick, because the check has no basis to issue one |
| It never blocks | `destructive` requires typing `RUN`; it does not refuse. Stopping an operator from doing their job is not this app's role - making it impossible to reach by reflex is |
| Both flags before a delete is reported | `rm -r` prompts and `rm -f` cannot take a tree. Warning on either alone is noise, and noise is what teaches people to click through warnings |
| `/var/log` is caution, `/var` is destruction | Losing logs is bad and survivable. Spending the strongest word on it leaves nothing for `rm -rf /` |
| Rules follow the host's OS | A `del /f /s /q c:\` in a task aimed at Linux is a *broken* task, not a dangerous one, and flagging it would be the wrong warning. An unidentified host gets both rule sets, which is the cautious reading and the honest one |
| Word-boundary matching | `cat /var/run/reboot-required` and `grep shutdown /var/log/syslog` must not fire. A check with false positives is a check nobody reads - there is a test for each of these |
| The assessment runs on what was *typed* | Not on the wrapped form. Reporting the app's own `sudo` back as a finding of the operator's would be a lie |

A separate rule flags a password on the command line. It is not destructive at
all - it is here because arguments are readable through `ps` by every account on
the host, and this app sends secrets to stdin everywhere else. A task should not
become the exception by accident.

The built-in catalog is held to its own rules by test rather than by comment:
no entry may contain a package-manager install verb, and no entry may assess as
`destructive`. A shipped task may be disruptive - `restart-ssh` is, and validates
the config with `sshd -t` first precisely because a bad config takes the daemon
down with no way back in - but a built-in reaching the level that demands typed
confirmation should be a deliberate decision, not a passing test.

## Posture checks on connect ✅

Settings › Startup › **Check posture on connect**, off by default. Tiers 0–1
run by themselves the moment a host connects.

**At connect time, not at pane-open time.** The first cut ran the checks from an
effect inside the Audit pane, which is not "on connect" at all: nothing happened
until the tab was opened, and the tab may never be. `connect()` fires it now,
and the report lives in `auditCache` - keyed by host, cleared on the same four
events a terminal is - so the pane shows what already ran rather than owning it.
`markAttempted` means the connect-time run and the pane's catch-up (for a host
already connected when the preference was switched on) can never both fire.
Failure is silent by design: this is a background courtesy, and a toast about it
would interrupt whatever the operator connected to do.

**Unprivileged always.** It takes exactly the path the *Run without root* button
takes by hand, and the report names what it had to skip - the same note it shows
when elevation is declined. A sudo prompt that appears because a pane was opened
is a prompt nobody asked for, and a password sent on a schedule is not consent.

Off by default for the same reason the whole audit is opt-in: the checks are
read-only, but running anything at all on someone's server unasked is a decision
the operator makes once, deliberately.

## VPN detection is cached ✅

Three things wanted the same answer - the 30 s navbar poll, the host-row glyphs,
and *every* failed probe's explanation - and each call spawned up to six local
processes. A page of unreachable hosts fanned out into dozens of
`tailscale status` invocations all saying the same thing.

`vpn/cache.rs` is a TTL cache whose lock is held across the load, which is what
gives single-flight: a second caller waits for the first rather than starting
its own, then finds the entry fresh. The wait is bounded by the existing 3 s
`CLI_TIMEOUT`, so a wedged client delays callers rather than hanging them.

| Decision | Why |
|---|---|
| Two TTLs - statuses 10 s, Twingate resources 300 s | The two answers age differently. A client going up or down is what the pill exists to show; the resource list is administrator-defined and near-static |
| Statuses stay under the 30 s poll | So a scheduled poll always does real work, and only the burst a single render produces is absorbed |
| `Freshness::Forced` for the refresh button, `Cached` for the poll | A user pressing refresh must reach the clients themselves, or the cache has quietly disabled the button |
| A forced load that *started after the click* satisfies the click | Three impatient clicks cost one CLI round, not three |
| `explain_unreachable` now reads both from the cache | It also stopped calling `twingate::status()` separately - the detection pass it already needed contains it |
| Tailscale peers stay uncached | The import dialog is user-triggered and infrequent, and a stale peer list would offer machines that have left the tailnet |

Six cache tests, all against a call-counting loader - no VPN client anywhere
near them, as the rest of the module's tests already require.

---

## Tailscale peer import ✅

Import tailnet machines as saved hosts, from the Tailscale tab of the VPN page.

**No local API socket was needed.** `tailscale status --json` already carries
the `Peer` map, so this parses more of the payload the status check was
fetching anyway - no new dependency, no new platform surface, and `run_cli`
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
| Default auth is `agent`, **not** `none` | Tailscale SSH's server is Linux-only *and* opt-in, so `none` is wrong more often than right for a batch. Choosing it names the selected peers that cannot serve it, using the `OS` field. `status --json` carries **no** per-peer SSH capability - checked against a real tailnet, the peer fields are `Active, Addrs, AllowedIPs, Created, CurAddr, DNSName, ExitNode, …` with nothing SSH-shaped - so per-node detection is not possible from this source |
| Already-saved peers shown but not selectable | Matched on every address a peer answers to, so a second import cannot duplicate |
| Saved sequentially | Each save rewrites `hosts.json`; concurrent writes would race for the file |

Still read-only toward Tailscale: no login, no `tailscale up`, no daemon start.
All nine parser tests are mock-driven - no Tailscale installed, no real CLI.

### Auth method `none` ✅

Added because peer import shipped without it and was therefore incomplete:
**Tailscale SSH nodes could not be connected to at all.** Tailscale
authenticates the node over WireGuard before the SSH layer is reached, then
offers SSH's `none` method - its own docs note that *"Some SSH clients may fail
to connect to an SSH server using no authentication."* ParolaSSH was one of
them, since `AuthMethod` had only password/publickey/agent.

`russh` already had `authenticate_none`, so the change is an enum variant
threaded through `build_credentials` and `authenticate`. Two rules:

- **Never a fallback.** It is chosen per host, so "no credential was sent" is
  always something the operator asked for, never something the app decided.
- **A remembered password is still not sent** - asserted by a test that puts one
  in the vault and checks it stays there.

The form and the import dialog both say plainly that an ordinary sshd will
refuse this, and the failure message names Tailscale SSH as the case where it
does work.

Verified against the Windows VM over its **tailnet** address: it answers with
Windows OpenSSH 9.5 offering `publickey,password,keyboard-interactive` - the
same host key as its LAN address, and no `none`. Per Tailscale's docs the SSH
server component is *"only available on: Linux, macOS open source"*, so a
Windows peer can never accept `none` however the tailnet routes to it. Reaching
a node over Tailscale and being served **by** Tailscale are different things.

## Import from `~/.ssh/config` ✅

The config was already parsed for `IdentityFile`; `ssh/import.rs` reads the rest
of it into importable connections. Opened from the hosts table toolbar.

| Decision | Why |
|---|---|
| First matching value wins, in file order | OpenSSH's own rule. It is why a trailing `Host *` acts as a fallback and a leading one does not, and reproducing it is the only way an imported host matches what `ssh <alias>` does |
| Only concrete patterns become candidates | `Host *`, `web0?` and `!secret` describe a *set*. A set has no address to connect to, so those blocks supply defaults and never appear as rows |
| `exists: false` rather than an empty list | No config file is not the same answer as a config holding no single-machine blocks, and the dialog says which |
| `Include` is reported, not followed | Following it means resolving globs relative to two base directories and guarding against include loops. Saying plainly that included hosts are missing is honest and costs nothing |
| `ProxyCommand` is named, not silently dropped | It is the one directive whose absence would make an imported host fail for a reason the row never mentioned |
| Only the first `ProxyJump` hop is imported | Chained jumps import as one hop with a note, rather than quietly becoming a different topology |
| An unparseable `Port` falls back to 22 with a note | The rest of the block is still worth importing |
| Passwords are never read | The config holds none. Imported hosts default to the batch's auth method, except where the block names its own `IdentityFile` |

Aliases are matched to jump hosts in a **second pass**, after every selection is
saved: a jump host has no id until it exists. A selected entry whose jump host
is neither selected nor already saved is named in a warning and imported as a
direct connection rather than being refused.

## ProxyJump ✅

The audit has advised using `ProxyJump` since tier 1 shipped; the app can now do
it. `proxy_jump` on a host record holds **the id of another saved connection**,
not an ssh_config string.

| Decision | Why |
|---|---|
| A jump host is a saved connection, not a config string | It then has its own credentials, its own key policy, its own audit history. Parsing `user@host:port` into a second, weaker host model would duplicate all of it |
| The tunnel is `channel_open_direct_tcpip` + `connect_stream` | russh 0.62 has both. No new dependency, and the target's handshake is ordinary from there |
| The jump `Session` is held inside the one it carries | The tunnel *is* a channel of that session. Dropping the jump would close the connection riding on it, so `Session.jump` owns it and `close()` tears down inner-first |
| The target's host key is still checked under its own name | The tunnel changes how the bytes travel, not who is trusted |
| `trust_unknown` is false at every hop | The dialog asked about the *target*. That consent cannot stand in for a different machine's key, so an unknown jump host says to connect to it directly first, where the question can honestly be asked |
| A jump host's password comes from the vault only | Same reason. There is no second password prompt, and the error names the host to connect first |
| Chains are capped at 4 and loops are refused | `jump::resolve` walks the chain before anything is dialled. A loop is a config mistake, not a topology, and the form does not offer a host that already jumps through the one being edited |
| Probes stay direct | `probe_host` opens a TCP socket from this machine, so a host only reachable through a jump reads as offline. The form says so rather than routing probes through a session that may not exist yet |

Chain resolution is unit-tested (ordering, loops, self-reference, deleted jump
host, depth cap), and the tunnel is confirmed working against a real bastion.
There is still no `#[ignore]`d live test for it: the live suite takes one host
from the environment and a jump needs two, so the proof is manual.

## FIDO key honest refusal

FIDO security keys (`sk-ed25519`, `sk-ecdsa`) are detected correctly - the UI
shows a USB glyph and the right wording - but direct signing is not supported:
`ssh-key`'s `try_sign` does not handle SK keys, and actual signing needs a
CTAP2 conversation with the hardware token.

Rather than letting the user walk into a dead end, the connect dialog now
refuses SK keys at the public-key path and shows a message naming `ssh-add -K`
and the SSH agent method as the working route. The passphrase prompt that
previously appeared for hardware keys has been removed, and the Connect button
is disabled.

Full in-process FIDO auth is deferred - see the memory record for the
cross-platform breakdown (libfido2/Linux, IOKit/macOS, WebAuthn/Windows).

## Local port forwarding

`ssh -L` as a UI feature. A tunnel listens on a local TCP port and, for each
incoming connection, opens a `direct-tcpip` channel through the SSH session
to a remote target, then relays bytes bidirectionally.

| Decision | Why |
|---|---|
| Tunnels live on the session, not globally | Disconnecting a host stops its tunnels. A tunnel without a session has nothing to forward through |
| Local port 0 means "pick one" | The OS chooses a free port; the UI reports what it got |
| Remote host defaults to 127.0.0.1 | The common case is reaching a service on the server itself |
| Each connection gets its own `direct-tcpip` channel | Parallel connections are independent, and a slow one cannot block another |
| Tunnels are stopped on disconnect | `stop_all_tunnels` runs before shells and streams are drained, so the accept loop exits before the session closes |
| Active connection count is tracked | The pane shows how many connections are flowing through each tunnel |
| No remote forwarding yet | `tcpip_forward` and `server_channel_open_forwarded_tcpip` exist in russh but require wiring the Handler trait callback. Local forwarding covers the most common use case |

The Tunnels tab appears in the host feature nav beside Files. The form asks for
a local port (optional), remote host and remote port, and each running tunnel
shows its endpoints with a close button and active connection count.

## Open questions

| Question | State |
|---|---|
| Status dot semantics | ✅ Decided: four states. Green + halo = live session, amber = reachable but no session, red = unreachable, grey = never probed. Fixing this also fixed a real bug: the hosts table treated "reachable" as "connected" and showed a Disconnect button for hosts we held no session on. `onlineCount` became `connectedCount` and counts sessions only. |
| Per-host shell cap of 8 - right number? | ✅ Reviewed; stays at 8 |
| Does Services ship Linux-first, or both platforms together? | ✅ Both shipped together (cross-platform parity is the rule) |
| Should tab titles be renameable? `rename()` exists, no UI yet | ✅ Shipped. Double-click the tab, or the pencil beside the font and clear buttons; F2 works while the tab itself holds focus. Enter and blur commit, Escape reverts, and an empty name restores the `shell N` it opened with rather than leaving the tab holding a name you just tried to delete. Titles live in the store with everything else about a shell, so they die with the app - the same lifetime as the scrollback beside them |
| Two rows of tabs (feature nav + shell tabs) - acceptable? | ✅ Reviewed; kept |

---

## UI quirks reported 2026-07-27

| Quirk | State | Notes |
|---|---|---|
| Pinned actions column was see-through | ✅ | `theme.css` sets `--bs-table-bg: transparent`, and Bootstrap's `.table > :not(caption) > * > *` applies it at higher specificity than the bare `.datatable__sticky` class - so the pinned column never painted a background and the scrolled-under cells read straight through it. Fixed by matching that selector's shape, with the fill behind a `--datatable-sticky-bg` variable because most tables sit on a card and the Files pane does not |
| Journal did not scroll to the newest line | ✅ | The service history now opens at the bottom and a follow behaves like `tail -f`. Scrolling up wins until the reader comes back within 24 px of the bottom |
| Settings read as one long scroll | ✅ | Tabbed with the same `feature-nav` the host view uses: Appearance · Startup · Transfers · Terminal · Files · Logs |
| Long listing squeezed the feature tabs | ✅ | On a filling page (Files, Terminal) the chrome above the pane is a shrinkable flex item, and `.feature-nav` scrolls on the x axis - which makes it a scroll container with no minimum height, so `/dev` compressed the tabs to a sliver instead of scrolling inside the pane. The chrome is now `flex: 0 0 auto`, and `.app-main` stops scrolling when a filling page is present so the header cannot be scrolled off either |
| Chain icon stayed whole after disconnect | ✅ | Not an icon problem - `statusFor` returns `connected` when *either* the connection map or the last heartbeat says so, and `disconnect` only cleared the first. The stale heartbeat held the status for up to 30 s. `disconnect` now marks health disconnected too, carrying the previous `reachable` over: the session ended, the machine did not |
| VPN pill tooltip too verbose | ✅ | Was every client's full sentence joined with `·`. Now names the one carrying traffic plus an idle count, or lists the installed clients and "none connected". The multi-VPN conflict note stays long - it is a real warning |

## Navigation layout ✅

Settings › Appearance › Navigation layout: Auto (host machine OS) · macOS ·
Windows · Linux. Auto is the default and follows the machine.

Two things stay tied to the real host rather than the preference, because they
are facts about the window and not about the drawing: only a genuine macOS
window has native traffic lights, so only there do we draw none; and only a
genuine macOS window needs the left inset to clear them. Forcing macOS style on
Linux therefore draws our own traffic lights rather than pretending the window
has real ones.

## Host context menu ✅

Right-click a host - in the sidebar or in the hosts table - for connect /
disconnect, open terminal, power, open host, edit, delete. Same entries as the
row's ⋯ menu, built from the same `useHostActions`, so the connect flow is
still fixed in one place. Entries that need a live session stay visible and
disabled with "Connect first" rather than disappearing.

The sidebar owns its own dialog set: the table's live on a page that may not be
mounted when the menu is used. `useContextMenuGuard` already suppressed the
webview's own menu, so there was nothing to fight.

## Tray status ✅

Four states, drawn on the beam of the app icon:

| State | Icon | Means |
|---|---|---|
| Offline | no `>` | this machine has no route off itself |
| Online | white `>` | on a network, holding no session |
| Connected | violet `>` | at least one live SSH session |
| Connecting | violet `>`, blinking | a connect is in flight |

`src-tauri/src/tray.rs`, promoted out of the inline module in `lib.rs`.

**The state is derived in Rust, not pushed from the frontend.** A tray exists to
be useful while the window is hidden; if the UI owned the state, closing to tray
would freeze the icon at whatever it last said. `refresh()` reads
`SessionRegistry` directly and is called from connect, disconnect, and the
30-second heartbeat - which is also how a session that dropped on its own, or a
network coming back, reaches the icon.

**Offline means no route, not "nothing answered".** A firewalled server must not
read as the laptop being off Wi-Fi. `has_route()` does a UDP `connect`, which
only fixes the socket's peer address and sends no packet, so it asks the routing
table rather than the network.

**Connecting starts at the prompt, not at the submit.** Being asked for a
password is part of connecting from the operator's side, so the dialog declares
itself over `set_connect_pending` - Rust cannot see a dialog. This is the one
piece of tray state the frontend owns, and it is deliberately *separate* from
the in-flight count rather than sharing it, so the same host appearing in both
during a submit cannot cancel itself out. It is a set of host ids, not a
counter: a repeated "opened" cannot drift the state upward and strand the icon
blinking. The declaration is cleared from the effect's cleanup, so cancel,
close, and navigating away all leave by the same path a successful connect does,
and `clear_connect_pending` runs at app mount because a reload destroys the
dialogs but not the process.

**Connecting is a guard, not paired calls.** `ConnectingGuard` marks the connect
in flight for its lifetime, so an early `?` out of `connect_host` cannot leave
the tray blinking forever. It is a count, not a flag: dialling two hosts at once
and having the first land must not clear the second. The blink task carries a
generation number and stops the moment the state moves on, so a stale task from
a previous connect cannot keep toggling the icon.

The blink shipped dead: `init` managed the state as `Arc<TrayState>` while every
lookup asked for `TrayState`. `manage` keys by exact type, so all of them got
`None` - and because each falls back to a default rather than failing, the count
stayed 0 and `Connecting` was unreachable. The icon swaps still worked, since
those never read the state, which is why only the blink was missing. Managed
unwrapped now, with a `debug_assert` after `manage` so a repeat is loud.

Three PNGs, not four - connecting blinks the connected icon against the offline
one. Rendered from `design/tray/tray-template.svg` by
`scripts/generate-tray-icons.sh` (needs `rsvg-convert`), so the tray states stay
tied to the one icon master rather than drifting as separate drawings.

### The tooltip that never showed

`.tooltip("ParolaSSH")` was already set - **libappindicator drops it**, and
every Linux status-notifier host uses libappindicator. So does `.set_title`.
Nothing about the call was wrong and no amount of fixing it would have helped.

What does render there is the menu, so the status text is now also a **disabled
first menu item**, updated by the same `refresh()`. Windows and macOS get it on
hover from the tooltip; Linux reads it in the menu. The text carries the state
too - *"ParolaSSH - 2 hosts connected"* - since a 22-pixel icon can only say so
much, and a unit test asserts every state's label still names the app.

## App log ✅

There was no logging at all before this - no `tracing`, no `log`, not a
`println!`. `src-tauri/src/logging.rs` writes a tab-separated file in the
per-OS app log directory, rotated at 1 MiB keeping one previous generation,
owner-only at creation for the same reason `hosts.json` is: it names every
machine you administer.

Two rules hold at every call site:

| Rule | Why |
|---|---|
| No secrets | Passwords are `Zeroizing` at the IPC boundary precisely so they are not copied around. A log file is a copy that outlives the process |
| No remote output | A journal, a listing, or a command's stdout contains whatever the remote machine puts there - the same reason terminal output is addressed rather than broadcast. Log what was attempted and how it ended, never what came back |

Logged today: app start/exit, connect success and failure (target and reason,
never the credential), disconnect, power actions, service actions. Settings ›
Logs shows the tail with a level filter, text filter, copy, reveal, and clear.

---

## Testing

| Suite | Command | Count |
|---|---|---|
| Rust unit | `cargo test --lib` | 281 |
| Rust fixtures | `cargo test --test audit_fixtures` | 40 |
| Rust live (needs a VM) | see below | 11, all `#[ignore]`d · green on Linux **and** Windows |
| Frontend | `npx tsc --noEmit` | typecheck only |

The new feature modules follow the `power.rs` testing shape: command
construction is pure and asserted as exact strings per OS (including the
quoting/injection cases), and parsers are fed captured-or-invented fixture
literals - `systemctl --plain`, `sc query` CRLF blocks, `wevtutil /f:text`,
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

All 11 assert on both Linux and Windows - verified green against a Windows 11
VM (`OpenSSH_for_Windows_9.5`). What used to skip now asserts the *other*
platform's behaviour: Windows CPU needs no delta (`LoadPercentage` is
instantaneous), tier 1 is Unix-only so the report must assemble from tier 0
alone with `tier1_ran = false`, Windows power needs no password and no `sudo`,
and the reboot cancel is verified by a second `shutdown /a` failing with 1116.

`skip()` remains only for macOS/BSD, which no VM covers yet. A skip still
reports `ok` - libtest has no runtime "skipped" outcome - so prefer a per-OS
assertion over calling it.

The power test schedules a reboot 600 minutes out and cancels it, then asserts
`/run/systemd/shutdown/scheduled` is gone. Nothing in the suite reboots
anything.
