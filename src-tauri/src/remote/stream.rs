//! A long-running remote command, streamed to the webview.
//!
//! `Session::exec` caps every command at thirty seconds, which is wrong for
//! `journalctl -f`. This is the other path: no PTY, no timeout, output
//! forwarded as events until the command ends or the pane closes it.
//!
//! Security posture is inherited from `shell.rs` unchanged — addressed events,
//! batched output, UTF-8 reassembled with the same `Decoder`. The batching loop
//! is a deliberate copy rather than a shared generic: the two differ only in
//! payload shape, and threading a type parameter through reads worse.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use russh::client::Msg;
use russh::{ChannelMsg, ChannelWriteHalf, Sig};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use super::client::Session;
use super::registry::SessionRegistry;
use super::shell::Decoder;
use crate::ssh::{SshError, SshResult};

/// Emitted for every chunk of stream output.
pub const OUTPUT_EVENT: &str = "stream://output";
/// Emitted once, when the command ends.
pub const CLOSED_EVENT: &str = "stream://closed";

/// Identifies one stream for the lifetime of the process, for the same reason
/// shells carry ids.
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
    /// Stop the remote command and close the channel. TERM is best-effort:
    /// servers that ignore channel signals let the command die on SIGPIPE at
    /// its next write instead, so either way it is bounded.
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
///
/// `stdin` exists for `sudo -S`, which reads the password from it: the bytes
/// are sent and the input half closed immediately, so the command never waits
/// on a terminal that a PTY-less stream does not have. Callers with nothing to
/// send pass `None` and leave stdin open, as a followed journal expects.
pub async fn open(
    session: &Session,
    app: AppHandle,
    webview_label: String,
    host_id: String,
    command: &str,
    stdin: Option<&[u8]>,
) -> SshResult<StreamHandle> {
    let stream_id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
    let channel = session.open_channel().await?;

    channel
        .exec(true, command)
        .await
        .map_err(|error| SshError::Io(format!("The server refused the command: {error}")))?;

    let (mut reader, writer) = channel.split();

    if let Some(bytes) = stdin {
        writer
            .data_bytes(bytes.to_vec())
            .await
            .map_err(|error| SshError::Io(format!("Could not send input: {error}")))?;
        writer
            .eof()
            .await
            .map_err(|error| SshError::Io(format!("Could not close input: {error}")))?;
    }

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
                // Keep reading: output may still be in flight behind the status.
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
            host_id: host_id.clone(),
            stream_id,
            exit_code,
        },
    );

    // A command that ended by itself has to release its own slot. Until this
    // existed only `close_stream` removed the handle, so every stream that
    // finished on its own — a task, a journal whose unit stopped — stayed in
    // the map counting against `MAX_STREAMS_PER_HOST`, and the fifth one on a
    // host was refused with "too many streams open" while none were.
    //
    // The channel is already gone, so the handle is dropped rather than
    // closed. `try_state` because a test harness may not manage a registry.
    if let Some(registry) = app.try_state::<SessionRegistry>() {
        if let Some(live) = registry.get(&host_id) {
            live.remove_stream(stream_id);
        }
    }
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
