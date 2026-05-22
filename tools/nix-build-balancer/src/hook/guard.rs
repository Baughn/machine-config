//! Per-candidate directive guard for the Nix build-hook protocol.
//!
//! Every `try` candidate Nix sends MUST be answered with exactly one
//! directive (`# accept`, `# decline`, `# decline-permanently`, or
//! `# postpone`) on the hook's stderr. If the hook exits before emitting
//! one, Nix fails the daemon with "unexpected EOF reading a line".
//!
//! `DirectiveGuard` makes that invariant impossible to forget: it emits
//! `# decline` on Drop unless an explicit emission was made first. Code
//! paths can `?`-propagate freely without risking protocol corruption.

use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclineKind {
    Decline,
    DeclinePermanently,
    Postpone,
}

impl DeclineKind {
    fn line(self) -> &'static [u8] {
        match self {
            DeclineKind::Decline => b"# decline\n",
            DeclineKind::DeclinePermanently => b"# decline-permanently\n",
            DeclineKind::Postpone => b"# postpone\n",
        }
    }
}

/// Sink for directive bytes. Production uses stderr; tests use a capture
/// buffer.
pub trait DirectiveSink: Send {
    fn write_all(&mut self, bytes: &[u8]);
}

struct StderrSink;

impl DirectiveSink for StderrSink {
    fn write_all(&mut self, bytes: &[u8]) {
        let mut err = io::stderr().lock();
        let _ = err.write_all(bytes);
        let _ = err.flush();
    }
}

pub struct DirectiveGuard {
    emitted: bool,
    sink: Box<dyn DirectiveSink>,
}

impl Default for DirectiveGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl DirectiveGuard {
    pub fn new() -> Self {
        Self {
            emitted: false,
            sink: Box::new(StderrSink),
        }
    }

    pub fn with_sink(sink: Box<dyn DirectiveSink>) -> Self {
        Self {
            emitted: false,
            sink,
        }
    }

    pub fn decline(&mut self) {
        self.emit_decline(DeclineKind::Decline);
    }

    pub fn emit_decline(&mut self, kind: DeclineKind) {
        if self.emitted {
            return;
        }
        self.sink.write_all(kind.line());
        self.emitted = true;
    }

    pub fn accept(&mut self, store_uri: &str) {
        if self.emitted {
            return;
        }
        let mut line = String::with_capacity(store_uri.len() + 16);
        line.push_str("# accept\n");
        line.push_str(store_uri);
        line.push('\n');
        self.sink.write_all(line.as_bytes());
        self.emitted = true;
    }
}

impl Drop for DirectiveGuard {
    fn drop(&mut self) {
        if !self.emitted {
            // Safety net: never let Nix read EOF while waiting for a directive.
            self.sink.write_all(DeclineKind::Decline.line());
            self.emitted = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default, Clone)]
    struct CaptureSink {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl CaptureSink {
        fn output(&self) -> String {
            String::from_utf8(self.bytes.lock().unwrap().clone()).unwrap()
        }
    }

    impl DirectiveSink for CaptureSink {
        fn write_all(&mut self, bytes: &[u8]) {
            self.bytes.lock().unwrap().extend_from_slice(bytes);
        }
    }

    fn capture() -> (DirectiveGuard, CaptureSink) {
        let sink = CaptureSink::default();
        (DirectiveGuard::with_sink(Box::new(sink.clone())), sink)
    }

    #[test]
    fn explicit_decline_emits_one_line() {
        let (mut g, sink) = capture();
        g.decline();
        drop(g);
        assert_eq!(sink.output(), "# decline\n");
    }

    #[test]
    fn explicit_accept_emits_two_lines() {
        let (mut g, sink) = capture();
        g.accept("ssh-ng://tsugumi");
        drop(g);
        assert_eq!(sink.output(), "# accept\nssh-ng://tsugumi\n");
    }

    #[test]
    fn decline_kinds_emit_correct_directives() {
        for (kind, expected) in [
            (DeclineKind::Decline, "# decline\n"),
            (DeclineKind::DeclinePermanently, "# decline-permanently\n"),
            (DeclineKind::Postpone, "# postpone\n"),
        ] {
            let (mut g, sink) = capture();
            g.emit_decline(kind);
            drop(g);
            assert_eq!(sink.output(), expected);
        }
    }

    #[test]
    fn drop_without_emission_falls_back_to_decline() {
        let (g, sink) = capture();
        drop(g);
        assert_eq!(
            sink.output(),
            "# decline\n",
            "guard MUST emit a directive on drop"
        );
    }

    #[test]
    fn second_emission_is_no_op() {
        let (mut g, sink) = capture();
        g.accept("ssh-ng://first");
        g.decline();
        g.accept("ssh-ng://second");
        drop(g);
        assert_eq!(
            sink.output(),
            "# accept\nssh-ng://first\n",
            "only the first emission counts"
        );
    }

    #[test]
    fn drop_after_emission_does_not_double_emit() {
        let (mut g, sink) = capture();
        g.emit_decline(DeclineKind::Postpone);
        drop(g);
        assert_eq!(sink.output(), "# postpone\n");
    }
}
