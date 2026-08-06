use std::fmt;

/// Stable public failure classes used by the `vesper` process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CliErrorKind {
    Usage = 2,
    ManifestOrPackage = 3,
    Compatibility = 4,
    Conformance = 5,
    Worker = 6,
}

impl CliErrorKind {
    pub const fn exit_code(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    kind: CliErrorKind,
    message: String,
}

impl CliError {
    pub fn new(kind: CliErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(CliErrorKind::Usage, message)
    }

    pub fn manifest_or_package(message: impl Into<String>) -> Self {
        Self::new(CliErrorKind::ManifestOrPackage, message)
    }

    pub fn compatibility(message: impl Into<String>) -> Self {
        Self::new(CliErrorKind::Compatibility, message)
    }

    pub fn conformance(message: impl Into<String>) -> Self {
        Self::new(CliErrorKind::Conformance, message)
    }

    pub fn worker(message: impl Into<String>) -> Self {
        Self::new(CliErrorKind::Worker, message)
    }

    pub const fn kind(&self) -> CliErrorKind {
        self.kind
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

impl From<String> for CliError {
    fn from(message: String) -> Self {
        Self::manifest_or_package(message)
    }
}

impl From<&str> for CliError {
    fn from(message: &str) -> Self {
        Self::manifest_or_package(message)
    }
}

pub type CliResult<T> = Result<T, CliError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_error_kinds_own_their_exit_codes() {
        assert_eq!(CliErrorKind::Usage.exit_code(), 2);
        assert_eq!(CliErrorKind::ManifestOrPackage.exit_code(), 3);
        assert_eq!(CliErrorKind::Compatibility.exit_code(), 4);
        assert_eq!(CliErrorKind::Conformance.exit_code(), 5);
        assert_eq!(CliErrorKind::Worker.exit_code(), 6);
    }
}
