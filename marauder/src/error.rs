use std::fmt;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Handle was invalid: {0}")]
    Handle(u32),
    #[error(transparent)]
    Os(#[from] std::io::Error),
    #[error("Error converting C string to a rust &str")]
    StringConversion(#[from] std::str::Utf8Error),
    #[error("Process not found")]
    ProcessNotFound,
    #[error(transparent)]
    NulError(#[from] std::ffi::NulError),
    #[error("Couldn't find function in the process: {0}")]
    ProcessAddress(u32),
    #[error("Error allocating or deallocting: {0}")]
    Allocation(u32),
    #[error("Error pertaining to memory access: {0}")]
    MemoryError(u32),
    #[error("Partial memory access: expected {expected} bytes, got {actual}")]
    PartialMemoryAccess { expected: usize, actual: usize },
    #[error("Memory access size overflow")]
    MemorySizeOverflow,
    #[error("String contains an interior NUL byte")]
    InteriorNul,
    #[error("Pattern error: {0}")]
    Pattern(String),
    #[error("Error pertaining to processes: {0}")]
    ProcessError(u32),
    #[error("Failed to enumerate {kind} snapshot for process {process_id} during {stage}")]
    Snapshot {
        kind: SnapshotKind,
        process_id: u32,
        stage: SnapshotStage,
        #[source]
        source: Box<Error>,
    },
    #[error("Timeout error")]
    Timeout,
    #[error("DLL path doesn't exist")]
    DllPath,
    #[error("Unsupported injector feature: {0}")]
    UnsupportedInjectorFeature(&'static str),
    #[error("DLL injection failed")]
    InjectionFailed,
    #[error("You must enable the feature for that render type")]
    RenderType,
    #[error("Couldn't allocate a console for the process: {0}")]
    ConsoleAllocation(u32),
    #[error("Unable to deallocate the console from the process: {0}")]
    ConsoleDeallocation(u32),
    #[error("Failed to create a DirectX dummy device")]
    DummyDevice,
    #[error("Hook index was out of bounds")]
    HookIndex,
    #[error("Hook is already installed for this method")]
    HookAlreadyInstalled,
    #[error("Hook target is not writable")]
    HookProtection,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SnapshotKind {
    Process,
    Module,
}

impl fmt::Display for SnapshotKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process => f.write_str("process"),
            Self::Module => f.write_str("module"),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SnapshotStage {
    Create,
    FirstEntry,
}

impl fmt::Display for SnapshotStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create => f.write_str("snapshot creation"),
            Self::FirstEntry => f.write_str("first entry lookup"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;

    use super::*;

    #[test]
    fn snapshot_error_preserves_context_and_source() {
        let error = Error::Snapshot {
            kind: SnapshotKind::Module,
            process_id: 42,
            stage: SnapshotStage::Create,
            source: Box::new(Error::MemoryError(5)),
        };

        assert_eq!(
            error.to_string(),
            "Failed to enumerate module snapshot for process 42 during snapshot creation"
        );
        assert_eq!(
            error.source().expect("snapshot source").to_string(),
            "Error pertaining to memory access: 5"
        );
    }
}
