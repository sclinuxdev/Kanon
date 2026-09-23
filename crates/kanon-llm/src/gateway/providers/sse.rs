//! Lightweight Server-Sent Events (SSE) stream decoder.
//!
//! Provides a zero-dependency, streaming frame decoder for HTTP/1.1 and HTTP/2
//! Server-Sent Events lines. Designed to safely handle TCP segmentation, CRLF/LF line endings,
//! and multi-line data payloads with minimal allocations.

/// An individual Server-Sent Event frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    /// Optional event name emitted via `event: <name>`.
    pub event: Option<String>,
    /// Accumulated data payload emitted via `data: <payload>`.
    pub data: String,
}

/// Streaming line buffer and event assembler.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: String,
    current_event: Option<String>,
    current_data: Vec<String>,
}

impl SseDecoder {
    /// Creates a new, empty `SseDecoder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a raw byte slice into the decoder, yielding any fully framed SSE events.
    pub fn decode(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let text = String::from_utf8_lossy(chunk);
        self.buffer.push_str(&text);

        let mut events = Vec::new();
        while let Some(pos) = self.buffer.find('\n') {
            let mut line = self.buffer[..pos].to_string();
            self.buffer.drain(..=pos);

            // Strip trailing carriage return if CRLF line ending was used
            if line.ends_with('\r') {
                line.pop();
            }

            if line.is_empty() {
                // An empty line marks the end of an event block according to the SSE specification
                if !self.current_data.is_empty() {
                    let data = self.current_data.join("\n");
                    events.push(SseEvent {
                        event: self.current_event.take(),
                        data,
                    });
                    self.current_data.clear();
                } else {
                    self.current_event = None;
                }
            } else if let Some(stripped) = line.strip_prefix("data:") {
                let trimmed = stripped.strip_prefix(' ').unwrap_or(stripped);
                self.current_data.push(trimmed.to_string());
            } else if let Some(stripped) = line.strip_prefix("event:") {
                let trimmed = stripped.strip_prefix(' ').unwrap_or(stripped);
                self.current_event = Some(trimmed.to_string());
            }
            // Comments (starting with ':') and other fields (id, retry) are safely ignored
        }

        events
    }

    /// Flushes any pending event if the stream terminates without a final trailing newline.
    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.current_data.is_empty() {
            let data = self.current_data.join("\n");
            self.current_data.clear();
            Some(SseEvent {
                event: self.current_event.take(),
                data,
            })
        } else {
            None
        }
    }
}
