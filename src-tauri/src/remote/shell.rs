//! An interactive shell, streamed to the webview.
//!
//! The channel is split: a background task owns the read half and forwards
//! output as Tauri events, while the write half stays in the registry so
//! keystrokes and window resizes can be sent from a command. Streaming rather
//! than polling is what makes `top` and `vim` behave.
//!
//! Three things here are security- rather than feature-driven:
//!
//! * **Events are addressed, not broadcast.** Terminal output can contain
//!   anything the user chose to `cat`, so it goes only to the webview that
//!   asked for the shell rather than to every window in the app.
//! * **Output is batched.** A remote `yes` produces packets faster than the
//!   IPC bridge can carry them, and an unbounded emit-per-packet loop would
//!   wedge the UI. Batching bounds the event rate without dropping bytes.
//! * **UTF-8 is reassembled across packets.** A multi-byte character can
//!   straddle two packets, and decoding each independently would corrupt it.

use std::sync::Arc;
use std::time::Duration;

use russh::client::Msg;
use russh::{ChannelMsg, ChannelWriteHalf, Pty};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use super::client::Session;
use crate::ssh::{SshError, SshResult};

/// Emitted for every chunk of terminal output.
pub const OUTPUT_EVENT: &str = "terminal://output";
/// Emitted once, when the shell ends.
pub const CLOSED_EVENT: &str = "terminal://closed";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputEvent {
    host_id: String,
    /// `true` for stderr, which xterm renders inline like a real terminal.
    stderr: bool,
    chunk: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClosedEvent {
    host_id: String,
    exit_code: Option<u32>,
}

/// The write end of a running shell.
pub struct ShellHandle {
    writer: Arc<ChannelWriteHalf<Msg>>,
}

impl ShellHandle {
    /// Send keystrokes.
    pub async fn write(&self, data: &str) -> SshResult<()> {
        self.writer
            .data_bytes(data.as_bytes().to_vec())
            .await
            .map_err(|error| SshError::Io(format!("Could not send input to the shell: {error}")))
    }

    /// Tell the remote side the window changed, so full-screen programs redraw
    /// at the right size instead of wrapping at 80 columns forever.
    pub async fn resize(&self, cols: u32, rows: u32) -> SshResult<()> {
        self.writer
            .window_change(cols.max(1), rows.max(1), 0, 0)
            .await
            .map_err(|error| SshError::Io(format!("Could not resize the terminal: {error}")))
    }

    pub async fn close(&self) {
        let _ = self.writer.close().await;
    }
}

/// How long output is allowed to accumulate before it is sent.
///
/// About one frame. Short enough that typing feels immediate, long enough that
/// a flood of small packets becomes one event instead of thousands.
const FLUSH_WINDOW: Duration = Duration::from_millis(16);

/// Force a flush once a batch reaches this size, so a fast producer does not
/// sit in the buffer growing for the whole window.
const MAX_BATCH: usize = 64 * 1024;

/// Open a PTY-backed login shell and start streaming it.
///
/// `webview_label` names the window that will receive the output events.
pub async fn open(
    session: &Session,
    app: AppHandle,
    webview_label: String,
    host_id: String,
    cols: u32,
    rows: u32,
) -> SshResult<ShellHandle> {
    let channel = session.open_channel().await?;

    // `xterm-256color` matches what the frontend renders, so remote programs
    // pick colours the terminal can actually show.
    channel
        .request_pty(
            true,
            "xterm-256color",
            cols.max(1),
            rows.max(1),
            0,
            0,
            &[(Pty::ECHO, 1), (Pty::TTY_OP_ISPEED, 38400), (Pty::TTY_OP_OSPEED, 38400)],
        )
        .await
        .map_err(|error| SshError::Io(format!("The server refused a PTY: {error}")))?;

    channel
        .request_shell(true)
        .await
        .map_err(|error| SshError::Io(format!("The server refused a shell: {error}")))?;

    let (mut reader, writer) = channel.split();

    // Decoded text goes to the batching task rather than straight out as an
    // event, so the reader never blocks on the IPC bridge.
    let (sender, receiver) = mpsc::unbounded_channel::<ShellMsg>();

    tokio::spawn(batch_and_emit(
        app,
        webview_label,
        host_id.clone(),
        receiver,
    ));

    tokio::spawn(async move {
        let mut stdout = Decoder::default();
        let mut stderr = Decoder::default();

        while let Some(message) = reader.wait().await {
            let decoded = match message {
                ChannelMsg::Data { ref data } => stdout.push(data).map(ShellMsg::Out),
                ChannelMsg::ExtendedData { ref data, ext } if ext == 1 => {
                    stderr.push(data).map(ShellMsg::Err)
                }
                // Keep reading after this: output may still be in flight
                // behind the status, and dropping it would truncate the last
                // line of whatever the user ran.
                ChannelMsg::ExitStatus { exit_status } => Some(ShellMsg::Exit(exit_status)),
                _ => None,
            };

            // A send failure means the emitter is gone, so nothing is
            // listening and there is no point draining the rest.
            if let Some(message) = decoded {
                if sender.send(message).is_err() {
                    break;
                }
            }
        }

        // Dropping the sender ends the batching task, which emits whatever is
        // still buffered before reporting the close.
        drop(sender);
    });

    Ok(ShellHandle {
        writer: Arc::new(writer),
    })
}

/// Coalesce output into at most one event per stream per `FLUSH_WINDOW`.
async fn batch_and_emit(
    app: AppHandle,
    webview_label: String,
    host_id: String,
    mut receiver: mpsc::UnboundedReceiver<ShellMsg>,
) {
    let emit = |stderr: bool, chunk: String| {
        if chunk.is_empty() {
            return;
        }
        // Addressed to one webview: output may contain anything the user
        // displayed, and other windows have no business seeing it.
        let _ = app.emit_to(
            webview_label.as_str(),
            OUTPUT_EVENT,
            OutputEvent {
                host_id: host_id.clone(),
                stderr,
                chunk,
            },
        );
    };

    let mut exit_code = None;

    loop {
        // Block until there is something to send, so an idle shell costs
        // nothing rather than waking every 16 ms.
        let Some(first) = receiver.recv().await else {
            break;
        };

        let mut out = String::new();
        let mut err = String::new();
        take(first, &mut out, &mut err, &mut exit_code);

        // Then take everything that arrives within the window.
        let deadline = tokio::time::Instant::now() + FLUSH_WINDOW;
        loop {
            if out.len() + err.len() >= MAX_BATCH {
                break;
            }
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
                Ok(Some(message)) => take(message, &mut out, &mut err, &mut exit_code),
                // Sender dropped, or the window elapsed: send what we have.
                Ok(None) | Err(_) => break,
            }
        }

        emit(false, out);
        emit(true, err);
    }

    let _ = app.emit_to(
        webview_label.as_str(),
        CLOSED_EVENT,
        ClosedEvent { host_id, exit_code },
    );
}

/// One message from the reader task.
enum ShellMsg {
    Out(String),
    Err(String),
    Exit(u32),
}

fn take(
    message: ShellMsg,
    out: &mut String,
    err: &mut String,
    exit_code: &mut Option<u32>,
) {
    match message {
        ShellMsg::Out(chunk) => out.push_str(&chunk),
        ShellMsg::Err(chunk) => err.push_str(&chunk),
        ShellMsg::Exit(code) => *exit_code = Some(code),
    }
}

/// Reassembles UTF-8 across packet boundaries.
#[derive(Default)]
struct Decoder {
    pending: Vec<u8>,
}

impl Decoder {
    /// Append bytes and return whatever is now completely decodable.
    fn push(&mut self, bytes: &[u8]) -> Option<String> {
        self.pending.extend_from_slice(bytes);

        let (text, consumed) = match std::str::from_utf8(&self.pending) {
            Ok(text) => (text.to_string(), self.pending.len()),
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                // SAFETY-adjacent: `valid_up_to` is by definition a valid
                // boundary, so this slice always decodes.
                let text = String::from_utf8_lossy(&self.pending[..valid_up_to]).to_string();

                match error.error_len() {
                    // Genuinely invalid bytes: emit a replacement and move on,
                    // otherwise the stream would stall on them forever.
                    Some(length) => (
                        format!("{text}\u{FFFD}"),
                        valid_up_to + length,
                    ),
                    // Truncated but valid so far — keep the tail for next time.
                    None => (text, valid_up_to),
                }
            }
        };

        self.pending.drain(..consumed);

        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_plain_ascii_straight_through() {
        let mut decoder = Decoder::default();
        assert_eq!(decoder.push(b"hello").as_deref(), Some("hello"));
        assert_eq!(decoder.push(b" world").as_deref(), Some(" world"));
    }

    #[test]
    fn reassembles_a_character_split_across_packets() {
        let mut decoder = Decoder::default();
        // "é" is 0xC3 0xA9 — arriving one byte per packet.
        assert_eq!(decoder.push(&[0xC3]), None, "half a character must not emit");
        assert_eq!(decoder.push(&[0xA9]).as_deref(), Some("é"));
    }

    #[test]
    fn holds_back_only_the_incomplete_tail() {
        let mut decoder = Decoder::default();
        // "ok" plus the first byte of a two-byte character.
        assert_eq!(decoder.push(&[b'o', b'k', 0xC3]).as_deref(), Some("ok"));
        assert_eq!(decoder.push(&[0xA9, b'!']).as_deref(), Some("é!"));
    }

    #[test]
    fn survives_genuinely_invalid_bytes() {
        let mut decoder = Decoder::default();
        // 0xFF is never valid UTF-8; the stream must not stall on it.
        assert_eq!(decoder.push(&[b'a', 0xFF, b'b']).as_deref(), Some("a\u{FFFD}"));
        // The `b` was not consumed with the bad byte, so it leads the next chunk.
        assert_eq!(decoder.push(b"c").as_deref(), Some("bc"));
    }

    #[test]
    fn keeps_ansi_escape_sequences_intact() {
        let mut decoder = Decoder::default();
        // Colour codes must survive untouched or the terminal renders garbage.
        let escape = "\x1b[31mred\x1b[0m";
        assert_eq!(decoder.push(escape.as_bytes()).as_deref(), Some(escape));
    }
}
