use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const LOG_LINES: usize = 2000;

#[derive(Default)]
struct Lines {
    lines: VecDeque<String>,
    next: u64,
}

#[derive(Clone, Default)]
pub struct ServerLog {
    inner: Arc<Mutex<Lines>>,
}

impl std::fmt::Debug for ServerLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerLog").field("next", &self.since(u64::MAX).1).finish()
    }
}

impl ServerLog {
    pub fn push(&self, line: impl Into<String>) {
        let Ok(mut inner) = self.inner.lock() else { return };
        inner.lines.push_back(line.into());
        inner.next += 1;
        while inner.lines.len() > LOG_LINES {
            inner.lines.pop_front();
        }
    }

    pub fn since(&self, since: u64) -> (Vec<String>, u64) {
        let Ok(inner) = self.inner.lock() else { return (Vec::new(), 0) };
        let first = inner.next - inner.lines.len() as u64;
        let skip = since.saturating_sub(first).min(inner.lines.len() as u64) as usize;
        (inner.lines.iter().skip(skip).cloned().collect(), inner.next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_lines_and_reads_from_a_number() {
        let log = ServerLog::default();
        assert_eq!(log.since(0), (Vec::new(), 0));
        for index in 0..LOG_LINES + 5 {
            log.push(format!("line {index}"));
        }
        let (lines, next) = log.since(0);
        assert_eq!(next, (LOG_LINES + 5) as u64);
        assert_eq!(lines.len(), LOG_LINES);
        assert_eq!(lines[0], "line 5");
        let (lines, _) = log.since(next - 2);
        assert_eq!(lines, [format!("line {}", LOG_LINES + 3), format!("line {}", LOG_LINES + 4)]);
        assert!(log.since(next + 10).0.is_empty());
    }
}
