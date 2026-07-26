//! Connecting and authenticating.
//!
//! The host key decision happens in `Handler::check_server_key` during the key
//! exchange, before any credential is sent, so refusing there means a password
//! never leaves the machine.
//!
//! That callback cannot block on a dialog, so an unknown host fails the
//! connection and reports its fingerprint; a second attempt carries
//! `trust_unknown` to record the key.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handle};
use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts_path};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{ChannelMsg, Disconnect};
use serde::Serialize;
use zeroize::Zeroizing;

use super::CommandOutput;
use crate::ssh::paths::SshPaths;
use crate::ssh::{SshError, SshResult};

/// How long to wait for the TCP connect and key exchange.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a one-shot command may run before we give up on it.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Where to connect.
#[derive(Debug, Clone)]
pub struct Target {
    pub hostname: String,
    pub port: u16,
    pub username: String,
}

/// How to authenticate. Secrets are held in `Zeroizing` so they are wiped when
/// the value is dropped rather than left in freed heap memory.
#[derive(Clone)]
pub enum Credentials {
    Password(Zeroizing<String>),
    Key {
        path: String,
        passphrase: Option<Zeroizing<String>>,
    },
    Agent,
}

/// Why a host key was rejected, so the UI can offer the right next step.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyProblem {
    /// `unknown` — never seen before. `changed` — does not match known_hosts.
    pub kind: String,
    pub fingerprint: String,
    pub algorithm: String,
}

/// The outcome of the host key check, recorded by the handler for the caller.
#[derive(Default)]
struct KeyVerdict {
    problem: Option<HostKeyProblem>,
    accepted_fingerprint: Option<String>,
}

/// What the key exchange negotiated, kept for the audit tab's tier-0 checks.
///
/// With russh's default `Preferred` lists, no SHA-1 kex or CBC cipher can be
/// negotiated at all, so only the host key algorithm and strict-kex checks can
/// fire today. The rest are kept in case those lists are widened for legacy
/// devices.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NegotiatedCrypto {
    pub kex: String,
    pub host_key_algorithm: String,
    pub cipher: String,
    pub client_mac: String,
    pub server_mac: String,
    pub strict_kex: bool,
}

struct Handler {
    hostname: String,
    port: u16,
    known_hosts: std::path::PathBuf,
    /// Set when the user has already seen and accepted this host's key.
    trust_unknown: bool,
    verdict: Arc<Mutex<KeyVerdict>>,
    /// Filled by `kex_done`, read by `connect` — same shape as `verdict`.
    crypto: Arc<Mutex<Option<NegotiatedCrypto>>>,
}

impl client::Handler for Handler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .fingerprint(Default::default())
            .to_string();
        let algorithm = server_public_key.algorithm().to_string();

        let known = check_known_hosts_path(
            &self.hostname,
            self.port,
            server_public_key,
            &self.known_hosts,
        );

        let problem = match known {
            // Recorded and matching.
            Ok(true) => None,
            // Not recorded. Trust it only if the user already said so.
            Ok(false) if self.trust_unknown => {
                // Failing to record just means we ask again next time.
                let _ = learn_known_hosts_path(
                    &self.hostname,
                    self.port,
                    server_public_key,
                    &self.known_hosts,
                );
                None
            }
            Ok(false) => Some(HostKeyProblem {
                kind: "unknown".into(),
                fingerprint: fingerprint.clone(),
                algorithm,
            }),
            // A recorded key that no longer matches — never auto-accepted, not
            // even with `trust_unknown`, which answered about a different key.
            Err(russh::keys::Error::KeyChanged { .. }) => Some(HostKeyProblem {
                kind: "changed".into(),
                fingerprint: fingerprint.clone(),
                algorithm,
            }),
            // Unreadable or absent known_hosts: treat as never seen.
            Err(_) if self.trust_unknown => {
                let _ = learn_known_hosts_path(
                    &self.hostname,
                    self.port,
                    server_public_key,
                    &self.known_hosts,
                );
                None
            }
            Err(_) => Some(HostKeyProblem {
                kind: "unknown".into(),
                fingerprint: fingerprint.clone(),
                algorithm,
            }),
        };

        let accepted = problem.is_none();
        if let Ok(mut verdict) = self.verdict.lock() {
            verdict.problem = problem;
            if accepted {
                verdict.accepted_fingerprint = Some(fingerprint);
            }
        }

        Ok(accepted)
    }

    /// Record what the key exchange settled on. Fires again on every rekey, but
    /// only the first observation is kept: russh does not re-signal strict-kex
    /// on a rekey, so only the initial handshake is fully described.
    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if let Ok(mut slot) = self.crypto.lock() {
            if slot.is_none() {
                *slot = Some(NegotiatedCrypto {
                    kex: names.kex.as_ref().to_string(),
                    host_key_algorithm: names.key.to_string(),
                    cipher: names.cipher.as_ref().to_string(),
                    client_mac: names.client_mac.as_ref().to_string(),
                    server_mac: names.server_mac.as_ref().to_string(),
                    strict_kex: names.strict_kex(),
                });
            }
        }
        Ok(())
    }
}

/// An authenticated connection.
pub struct Session {
    handle: Handle<Handler>,
    pub fingerprint: Option<String>,
    /// What the first key exchange negotiated. `None` only if russh never
    /// called `kex_done`, which would mean no handshake happened at all.
    pub negotiated: Option<NegotiatedCrypto>,
}

impl Session {
    /// Connect, verify the host key, and authenticate.
    pub async fn connect(
        target: &Target,
        credentials: &Credentials,
        trust_unknown: bool,
    ) -> SshResult<Self> {
        let known_hosts = SshPaths::discover()?.known_hosts();
        let verdict = Arc::new(Mutex::new(KeyVerdict::default()));
        let crypto = Arc::new(Mutex::new(None));

        let handler = Handler {
            hostname: target.hostname.clone(),
            port: target.port,
            known_hosts,
            trust_unknown,
            verdict: Arc::clone(&verdict),
            crypto: Arc::clone(&crypto),
        };

        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(3600)),
            ..Default::default()
        });

        let address = (target.hostname.as_str(), target.port);
        let connect = client::connect(config, address, handler);

        let mut handle = match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
            Err(_) => {
                return Err(SshError::Io(format!(
                    "{}:{} did not respond within {} seconds.",
                    target.hostname,
                    target.port,
                    CONNECT_TIMEOUT.as_secs()
                )))
            }
            Ok(Ok(handle)) => handle,
            Ok(Err(error)) => {
                // A rejected host key surfaces as a generic failure, so check
                // the handler's verdict before blaming the network.
                if let Some(problem) = take_problem(&verdict) {
                    return Err(host_key_error(target, &problem));
                }
                return Err(SshError::Io(format!(
                    "Could not connect to {}:{} — {error}",
                    target.hostname, target.port
                )));
            }
        };

        let authenticated = authenticate(&mut handle, target, credentials).await?;
        if !authenticated {
            return Err(SshError::invalid(auth_failure_message(credentials, target)));
        }

        let fingerprint = verdict
            .lock()
            .ok()
            .and_then(|verdict| verdict.accepted_fingerprint.clone());
        let negotiated = crypto.lock().ok().and_then(|slot| slot.clone());

        Ok(Self { handle, fingerprint, negotiated })
    }

    /// Run one command, optionally feeding it stdin, and collect its output.
    ///
    /// `stdin` exists for `sudo -S`: a password on the command line would be
    /// visible in the remote process list.
    pub async fn exec(&self, command: &str, stdin: Option<&[u8]>) -> SshResult<CommandOutput> {
        self.exec_with_timeout(command, stdin, COMMAND_TIMEOUT).await
    }

    /// `exec` with a caller-chosen ceiling, for the rare command that is
    /// legitimately slow (a Windows Update query can take minutes). Anything
    /// open-ended belongs on the `stream` path instead.
    pub async fn exec_with_timeout(
        &self,
        command: &str,
        stdin: Option<&[u8]>,
        timeout: Duration,
    ) -> SshResult<CommandOutput> {
        let run = self.exec_inner(command, stdin);
        match tokio::time::timeout(timeout, run).await {
            Ok(result) => result,
            Err(_) => Err(SshError::Io(format!(
                "The remote command did not finish within {} seconds.",
                timeout.as_secs()
            ))),
        }
    }

    async fn exec_inner(&self, command: &str, stdin: Option<&[u8]>) -> SshResult<CommandOutput> {
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|error| SshError::Io(format!("Could not open a session channel: {error}")))?;

        channel
            .exec(true, command)
            .await
            .map_err(|error| SshError::Io(format!("Could not start the remote command: {error}")))?;

        if let Some(bytes) = stdin {
            channel
                .data_bytes(bytes.to_vec())
                .await
                .map_err(|error| SshError::Io(format!("Could not send input: {error}")))?;
            channel
                .eof()
                .await
                .map_err(|error| SshError::Io(format!("Could not close input: {error}")))?;
        }

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = None;

        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { ref data } => stdout.extend_from_slice(data),
                // ext 1 is stderr; anything else is not defined for sessions.
                ChannelMsg::ExtendedData { ref data, ext } => {
                    if ext == 1 {
                        stderr.extend_from_slice(data);
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => {
                    // Keep reading: output may still be in flight.
                    exit_code = Some(exit_status);
                }
                _ => {}
            }
        }

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&stdout).to_string(),
            stderr: String::from_utf8_lossy(&stderr).to_string(),
            exit_code,
        })
    }

    /// Cheap liveness check for the heartbeat. Opens and closes a channel
    /// rather than running a command: one round trip, no remote process, and
    /// nothing in the auth log.
    pub async fn is_alive(&self) -> bool {
        let check = async {
            match self.handle.channel_open_session().await {
                Ok(channel) => {
                    let _ = channel.close().await;
                    true
                }
                Err(_) => false,
            }
        };

        tokio::time::timeout(Duration::from_secs(5), check)
            .await
            .unwrap_or(false)
    }

    /// Open a bare session channel for a caller that drives it itself, so
    /// `shell` can request a PTY without this module knowing about terminals.
    pub async fn open_channel(&self) -> SshResult<russh::Channel<client::Msg>> {
        self.handle
            .channel_open_session()
            .await
            .map_err(|error| SshError::Io(format!("Could not open a session channel: {error}")))
    }

    pub async fn close(&self) {
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;
    }
}

fn take_problem(verdict: &Arc<Mutex<KeyVerdict>>) -> Option<HostKeyProblem> {
    verdict.lock().ok().and_then(|mut v| v.problem.take())
}

/// Turn a host key rejection into a message that says what to do about it.
fn host_key_error(target: &Target, problem: &HostKeyProblem) -> SshError {
    if problem.kind == "changed" {
        return SshError::invalid(format!(
            "The host key for {}:{} has CHANGED.\n\nIt now offers {} ({}). \
             This is what an intercepted connection looks like. If you know \
             the server was rebuilt, remove its entry from ~/.ssh/known_hosts \
             and try again. Otherwise, do not connect.",
            target.hostname, target.port, problem.fingerprint, problem.algorithm
        ));
    }

    SshError::invalid(format!(
        "{}:{} is not in your known_hosts file.\n\nIt offers {} ({}). \
         Confirm this fingerprint out-of-band, then choose “Trust and connect” \
         to record it.\n\nHOSTKEY:{}",
        target.hostname, target.port, problem.fingerprint, problem.algorithm, problem.kind
    ))
}

async fn authenticate(
    handle: &mut Handle<Handler>,
    target: &Target,
    credentials: &Credentials,
) -> SshResult<bool> {
    match credentials {
        Credentials::Password(password) => handle
            .authenticate_password(target.username.clone(), password.as_str())
            .await
            .map(|result| result.success())
            .map_err(|error| SshError::Io(format!("Password authentication failed: {error}"))),

        Credentials::Key { path, passphrase } => {
            let expanded = crate::ssh::paths::expand_tilde(path);
            let key = load_secret_key(&expanded, passphrase.as_ref().map(|p| p.as_str())).map_err(
                |error| {
                    SshError::invalid(format!(
                        "Could not load {} — {error}. If the key has a passphrase, enter it.",
                        expanded.display()
                    ))
                },
            )?;

            let hash = handle
                .best_supported_rsa_hash()
                .await
                .map_err(|error| SshError::Io(format!("Could not negotiate a signature: {error}")))?
                .flatten();

            handle
                .authenticate_publickey(
                    target.username.clone(),
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await
                .map(|result| result.success())
                .map_err(|error| SshError::Io(format!("Key authentication failed: {error}")))
        }

        Credentials::Agent => authenticate_with_agent(handle, target).await,
    }
}

/// Connect to the agent, then offer everything it holds. Only the connection
/// differs per platform: `SSH_AUTH_SOCK` on Unix, a named pipe on Windows.
#[cfg(unix)]
async fn authenticate_with_agent(handle: &mut Handle<Handler>, target: &Target) -> SshResult<bool> {
    use russh::keys::agent::client::AgentClient;

    let mut agent = AgentClient::connect_env().await.map_err(|error| {
        SshError::invalid(format!(
            "Could not reach an SSH agent: {error}. Is SSH_AUTH_SOCK set?"
        ))
    })?;

    offer_agent_identities(handle, target, &mut agent).await
}

#[cfg(windows)]
async fn authenticate_with_agent(handle: &mut Handle<Handler>, target: &Target) -> SshResult<bool> {
    use russh::keys::agent::client::AgentClient;

    let mut agent = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
        .await
        .map_err(|error| {
            SshError::invalid(format!(
                "Could not reach the Windows OpenSSH agent: {error}. Start the \
                 “OpenSSH Authentication Agent” service, or use a key file instead."
            ))
        })?;

    offer_agent_identities(handle, target, &mut agent).await
}

/// Try each identity in turn until the server accepts one. A rejected key is
/// not an error — an agent commonly holds keys for several hosts.
async fn offer_agent_identities<S>(
    handle: &mut Handle<Handler>,
    target: &Target,
    agent: &mut russh::keys::agent::client::AgentClient<S>,
) -> SshResult<bool>
where
    S: russh::keys::agent::client::AgentStream + Unpin + Send,
{
    use russh::keys::agent::AgentIdentity;

    let identities = agent
        .request_identities()
        .await
        .map_err(|error| SshError::Io(format!("The agent would not list its keys: {error}")))?;

    if identities.is_empty() {
        return Err(SshError::invalid(
            "The SSH agent is running but holds no keys. Add one with `ssh-add`.",
        ));
    }

    for identity in identities {
        let accepted = match identity {
            AgentIdentity::PublicKey { key, .. } => handle
                .authenticate_publickey_with(target.username.clone(), key, None, agent)
                .await
                .map(|outcome| outcome.success()),
            // A certificate authenticates differently: the server checks the
            // CA signature rather than a key it already knows.
            AgentIdentity::Certificate { certificate, .. } => handle
                .authenticate_certificate_with(
                    target.username.clone(),
                    certificate,
                    None,
                    agent,
                )
                .await
                .map(|outcome| outcome.success()),
        };

        if accepted.unwrap_or(false) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn auth_failure_message(credentials: &Credentials, target: &Target) -> String {
    match credentials {
        Credentials::Password(_) => format!(
            "{} rejected the password for “{}”. Check the password, and that \
             the server allows password authentication.",
            target.hostname, target.username
        ),
        Credentials::Key { path, .. } => format!(
            "{} rejected the key {path} for “{}”. Make sure its public half is \
             in that account's ~/.ssh/authorized_keys.",
            target.hostname, target.username
        ),
        Credentials::Agent => format!(
            "{} rejected every key held by your SSH agent for “{}”.",
            target.hostname, target.username
        ),
    }
}
