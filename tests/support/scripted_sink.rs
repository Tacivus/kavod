use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;

pub enum SinkStep {
    Write(io::Result<usize>),
    Flush(io::Result<()>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SinkCall {
    Write {
        bytes: Vec<u8>,
        result: Result<usize, ()>,
    },
    Flush {
        result: Result<(), ()>,
    },
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SinkTrace {
    pub calls: Vec<SinkCall>,
    accepted_bytes: Vec<u8>,
    committed_len: usize,
}

impl SinkTrace {
    pub fn accepted_bytes(&self) -> &[u8] {
        &self.accepted_bytes
    }

    pub fn committed_bytes(&self) -> &[u8] {
        &self.accepted_bytes[..self.committed_len]
    }

    pub fn uncertain_suffix(&self) -> &[u8] {
        &self.accepted_bytes[self.committed_len..]
    }
}

pub type SharedSinkTrace = Rc<RefCell<SinkTrace>>;

pub struct ScriptedSink {
    steps: VecDeque<SinkStep>,
    trace: SharedSinkTrace,
}

impl ScriptedSink {
    pub fn new(steps: impl IntoIterator<Item = SinkStep>) -> (Self, SharedSinkTrace) {
        let trace = Rc::new(RefCell::new(SinkTrace::default()));
        (
            Self {
                steps: steps.into_iter().collect(),
                trace: Rc::clone(&trace),
            },
            trace,
        )
    }
}

impl io::Write for ScriptedSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = match self
            .steps
            .pop_front()
            .expect("each sink call must consume exactly one scripted result")
        {
            SinkStep::Write(result) => result,
            SinkStep::Flush(_) => panic!("a sink write call must consume a write result"),
        };
        let traced_result = result.as_ref().copied().map_err(|_| ());
        let mut trace = self.trace.borrow_mut();
        if let Ok(count) = result.as_ref()
            && *count <= bytes.len()
        {
            trace.accepted_bytes.extend_from_slice(&bytes[..*count]);
        }
        trace.calls.push(SinkCall::Write {
            bytes: bytes.to_vec(),
            result: traced_result,
        });
        drop(trace);
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        let result = match self
            .steps
            .pop_front()
            .expect("each sink call must consume exactly one scripted result")
        {
            SinkStep::Flush(result) => result,
            SinkStep::Write(_) => panic!("a sink flush call must consume a flush result"),
        };
        let traced_result = result.as_ref().map(|_| ()).map_err(|_| ());
        let mut trace = self.trace.borrow_mut();
        if result.is_ok() {
            trace.committed_len = trace.accepted_bytes.len();
        }
        trace.calls.push(SinkCall::Flush {
            result: traced_result,
        });
        drop(trace);
        result
    }
}
