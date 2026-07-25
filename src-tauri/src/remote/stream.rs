//! A long-running remote command, streamed to the webview.
//!
//! `Session::exec` deliberately caps every command at thirty seconds, which is
//! right for "run this and give me the output" and wrong for `journalctl -f`,
//! which by definition never finishes. This is the other path: no PTY, no
//! timeout, output forwarded as events until the command ends or the pane
//! closes it.
//!
//! The security posture is inherited from `shell.rs` unchanged: events are
//! **addressed** to the webview that asked (a followed log can contain
//! anything the machine writes to it), output is **batched** so a chatty
//! producer cannot wedge the IPC bridge, and **UTF-8 is reassembled** across
//! packet boundaries with the same `Decoder`.
//!
//! The batching loop is a deliberate copy of the one in `shell.rs` rather
//! than a shared generic: the two differ only in payload shape, and a type
//! parameter threaded through the emitter costs more to read than sixty
//! duplicated lines.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use russh::client::Msg;
use russh::{ChannelMsg, ChannelWriteHalf, Sig};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use super::client::Session;
use super::shell::Decoder;
use crate::ssh::{SshError, SshResult};

/// Emitted for every chunk of stream output.
pub const OUTPUT_EVENT: &str = "stream://output";
/// Emitted once, when the command ends.
pub const CLOSED_EVENT: &str = "stream://closed";

/// Identifies one stream for the lifetime of the process, for the same reason
/// shells carry ids: a pane must be able to tell its own output from that of
/// a stream it replaced, and close only its own.
static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputEvent {
    host_id: String,
    stream_id: u64,
    stderr: bool,
    chunk: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClosedEvent {
    host_id: String,
    stream_id: u64,
    exit_code: Option<u32>,
}

/// The handle kept in the registry while a stream runs.
pub struct StreamHandle {
    pub id: u64,
    writer: Arc<ChannelWriteHalf<Msg>>,
}

impl StreamHandle {
    /// Stop the remote command and close the channel.
    ///
    /// The TERM signal is best-effort: modern sshd delivers it and the
    /// command dies at once; older servers ignore channel signals, and then
    /// the command dies on SIGPIPE at its next write after the close. Either
    /// way it is bounded — nothing keeps running unobserved forever.
    pub async fn close(&self) {
        let _ = self.writer.signal(Sig::TERM).await;
        let _ = self.writer.close().await;
    }
}

/// Same coalescing constants as the terminal, for the same reason.
const FLUSH_WINDOW: Duration = Duration::from_millis(16);
const MAX_BATCH: usize = 64 * 1024;

/// Run `command` on the session and stream its output.
///
/// `webview_label` names the window that will receive the events.
pub async fn open(
    session: &Session,
    app: AppHandle,
    webview_label: String,
    host_id: String,
    command: &str,
) -> SshResult<StreamHandle> {
    let stream_id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
    let channel = session.open_channel().await?;

    channel
        .exec(true, command)
        .await
        .map_err(|error| SshError::Io(format!("The server refused the command: {error}")))?;

    let (mut reader, writer) = channel.split();

    let (sender, receiver) = mpsc::unbounded_channel::<StreamMsg>();

    tokio::spawn(batch_and_emit(
        app,
        webview_label,
        host_id.clone(),
        stream_id,
        receiver,
    ));

    tokio::spawn(async move {
        let mut stdout = Decoder::default();
        let mut stderr = Decoder::default();

        while let Some(message) = reader.wait().await {
            let decoded = match message {
                ChannelMsg::Data { ref data } => stdout.push(data).map(StreamMsg::Out),
                ChannelMsg::ExtendedData { ref data, ext } if ext == 1 => {
                    stderr.push(data).map(StreamMsg::Err)
                }
                // Keep reading after the status: output may still be in
                // flight behind it.
                ChannelMsg::ExitStatus { exit_status } => Some(StreamMsg::Exit(exit_status)),
                _ => None,
            };

            if let Some(message) = decoded {
                if sender.send(message).is_err() {
                    break;
                }
            }
        }

        drop(sender);
    });

    Ok(StreamHandle {
        id: stream_id,
        writer: Arc::new(writer),
    })
}

/// Coalesce output into at most one event per stream per `FLUSH_WINDOW`.
async fn batch_and_emit(
    app: AppHandle,
    webview_label: String,
    host_id: String,
    stream_id: u64,
    mut receiver: mpsc::UnboundedReceiver<StreamMsg>,
) {
    let emit = |stderr: bool, chunk: String| {
        if chunk.is_empty() {
            return;
        }
        let _ = app.emit_to(
            webview_label.as_str(),
            OUTPUT_EVENT,
            OutputEvent {
                host_id: host_id.clone(),
                stream_id,
                stderr,
                chunk,
            },
        );
    };

    let mut exit_code = None;

    loop {
        let Some(first) = receiver.recv().await else {
            break;
        };

        let mut out = String::new();
        let mut err = String::new();
        take(first, &mut out, &mut err, &mut exit_code);

        let deadline = tokio::time::Instant::now() + FLUSH_WINDOW;
        loop {
            if out.len() + err.len() >= MAX_BATCH {
                break;
            }
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
                Ok(Some(message)) => take(message, &mut out, &mut err, &mut exit_code),
                Ok(None) | Err(_) => break,
            }
        }

        emit(false, out);
        emit(true, err);
    }

    let _ = app.emit_to(
        webview_label.as_str(),
        CLOSED_EVENT,
        ClosedEvent {
            host_id,
            stream_id,
            exit_code,
        },
    );
}

enum StreamMsg {
    Out(String),
    Err(String),
    Exit(u32),
}

fn take(message: StreamMsg, out: &mut String, err: &mut String, exit_code: &mut Option<u32>) {
    match message {
        StreamMsg::Out(chunk) => out.push_str(&chunk),
        StreamMsg::Err(chunk) => err.push_str(&chunk),
        StreamMsg::Exit(code) => *exit_code = Some(code),
    }
}
