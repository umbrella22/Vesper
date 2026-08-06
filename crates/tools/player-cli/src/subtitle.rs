#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubtitleScope {
    Regression,
    Device,
    Complete,
}

impl SubtitleScope {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Regression => "regression",
            Self::Device => "device",
            Self::Complete => "complete",
        }
    }

    pub(crate) const fn includes_regression(self) -> bool {
        matches!(self, Self::Regression | Self::Complete)
    }

    pub(crate) const fn includes_device(self) -> bool {
        matches!(self, Self::Device | Self::Complete)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubtitleErrorKind {
    Usage,
    Storage,
    Compatibility,
    Conformance,
    Worker,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct SubtitleError {
    kind: SubtitleErrorKind,
    message: String,
}

impl SubtitleError {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self {
            kind: SubtitleErrorKind::Usage,
            message: message.into(),
        }
    }

    pub(crate) fn storage(message: impl Into<String>) -> Self {
        Self {
            kind: SubtitleErrorKind::Storage,
            message: message.into(),
        }
    }

    pub(crate) fn compatibility(message: impl Into<String>) -> Self {
        Self {
            kind: SubtitleErrorKind::Compatibility,
            message: message.into(),
        }
    }

    pub(crate) fn conformance(message: impl Into<String>) -> Self {
        Self {
            kind: SubtitleErrorKind::Conformance,
            message: message.into(),
        }
    }

    pub(crate) fn worker(message: impl Into<String>) -> Self {
        Self {
            kind: SubtitleErrorKind::Worker,
            message: message.into(),
        }
    }

    pub(crate) fn with_suffix(self, suffix: impl std::fmt::Display) -> Self {
        Self {
            kind: self.kind,
            message: format!("{}; {suffix}", self.message),
        }
    }

    pub(crate) const fn kind(&self) -> SubtitleErrorKind {
        self.kind
    }

    pub(crate) const fn exit_code(&self) -> i32 {
        match self.kind {
            SubtitleErrorKind::Usage => 2,
            SubtitleErrorKind::Storage => 3,
            SubtitleErrorKind::Compatibility => 4,
            SubtitleErrorKind::Conformance => 5,
            SubtitleErrorKind::Worker => 6,
        }
    }
}
