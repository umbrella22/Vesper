use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use player_cli::PluginDescriptor;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::plugin_inspection::{
    PluginInspectionOperation, PluginInspectionOutcome, PluginInspectionReport,
};

pub const PLUGIN_WORKER_PROTOCOL_VERSION: u32 = 1;
pub const MAX_PLUGIN_WORKER_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_PLUGIN_WORKER_RESPONSE_BYTES: usize = 1024 * 1024;
pub const PLUGIN_WORKER_START_GATE: &[u8; 8] = b"VSPRWRK1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginWorkerRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub operation: PluginInspectionOperation,
    pub library_path_utf8: String,
    pub descriptor: PluginDescriptor,
}

impl PluginWorkerRequest {
    pub fn new(
        request_id: u64,
        operation: PluginInspectionOperation,
        library_path_utf8: String,
        descriptor: PluginDescriptor,
    ) -> Self {
        Self {
            protocol_version: PLUGIN_WORKER_PROTOCOL_VERSION,
            request_id,
            operation,
            library_path_utf8,
            descriptor,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.protocol_version != PLUGIN_WORKER_PROTOCOL_VERSION {
            return Err(format!(
                "unsupported plugin worker request protocol version {}",
                self.protocol_version
            ));
        }
        if self.request_id == 0 {
            return Err("plugin worker request id must be nonzero".to_owned());
        }
        if self.library_path_utf8.is_empty() {
            return Err("plugin worker library path must not be empty".to_owned());
        }
        self.descriptor
            .validate()
            .map_err(|error| format!("invalid plugin worker descriptor: {error}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginWorkerResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: PluginInspectionOutcome,
    pub report: PluginInspectionReport,
}

impl PluginWorkerResponse {
    pub fn new(request_id: u64, report: PluginInspectionReport) -> Self {
        Self {
            protocol_version: PLUGIN_WORKER_PROTOCOL_VERSION,
            request_id,
            outcome: report.outcome(),
            report,
        }
    }

    #[cfg(any(unix, windows))]
    pub fn validate_for_request(&self, request_id: u64) -> Result<(), String> {
        if self.protocol_version != PLUGIN_WORKER_PROTOCOL_VERSION {
            return Err(format!(
                "unsupported plugin worker response protocol version {}",
                self.protocol_version
            ));
        }
        if self.request_id != request_id {
            return Err(format!(
                "plugin worker response id {} does not match request id {request_id}",
                self.request_id
            ));
        }
        if self.outcome != self.report.outcome() {
            return Err("plugin worker response outcome does not match its report".to_owned());
        }
        Ok(())
    }
}

pub fn read_worker_request(path: &Path) -> Result<PluginWorkerRequest, String> {
    read_bounded_json(
        path,
        MAX_PLUGIN_WORKER_REQUEST_BYTES,
        "plugin worker request",
    )
}

#[cfg(any(unix, windows))]
pub fn read_worker_response(path: &Path) -> Result<PluginWorkerResponse, String> {
    read_bounded_json(
        path,
        MAX_PLUGIN_WORKER_RESPONSE_BYTES,
        "plugin worker response",
    )
}

#[cfg(any(unix, windows))]
pub fn write_worker_request(path: &Path, request: &PluginWorkerRequest) -> Result<(), String> {
    write_bounded_json_new(
        path,
        request,
        MAX_PLUGIN_WORKER_REQUEST_BYTES,
        "plugin worker request",
    )
}

pub fn write_worker_response(path: &Path, response: &PluginWorkerResponse) -> Result<(), String> {
    write_bounded_json_new(
        path,
        response,
        MAX_PLUGIN_WORKER_RESPONSE_BYTES,
        "plugin worker response",
    )
}

fn read_bounded_json<T: DeserializeOwned>(
    path: &Path,
    maximum_bytes: usize,
    label: &str,
) -> Result<T, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect {label} '{}': {error}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "{label} '{}' is not a regular non-symlink file",
            path.display()
        ));
    }
    if metadata.len() > maximum_bytes as u64 {
        return Err(format!("{label} exceeds {maximum_bytes} bytes"));
    }
    let file = File::open(path)
        .map_err(|error| format!("failed to open {label} '{}': {error}", path.display()))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((maximum_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read {label}: {error}"))?;
    if bytes.len() > maximum_bytes {
        return Err(format!("{label} exceeds {maximum_bytes} bytes"));
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid {label} JSON: {error}"))
}

fn write_bounded_json_new<T: Serialize>(
    path: &Path,
    value: &T,
    maximum_bytes: usize,
    label: &str,
) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| format!("{label} path has no parent directory"))?;
    if !parent.is_dir() {
        return Err(format!(
            "{label} parent '{}' is not a directory",
            parent.display()
        ));
    }
    let temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("failed to create {label} staging file: {error}"))?;
    let mut writer = LimitedWriter::new(temporary, maximum_bytes);
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| format!("failed to serialize {label}: {error}"))?;
    writer
        .write_all(b"\n")
        .map_err(|error| format!("failed to finish {label}: {error}"))?;
    let temporary = writer.into_inner();
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("failed to sync {label}: {error}"))?;
    temporary.persist_noclobber(path).map_err(|error| {
        format!(
            "refusing to replace {label} '{}': {}",
            path.display(),
            error.error
        )
    })?;
    Ok(())
}

struct LimitedWriter<W> {
    inner: W,
    remaining: usize,
}

impl<W> LimitedWriter<W> {
    fn new(inner: W, maximum_bytes: usize) -> Self {
        Self {
            inner,
            remaining: maximum_bytes,
        }
    }

    fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other(
                "bounded JSON output exceeds its byte limit",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.remaining = self.remaining.saturating_sub(written);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_writer_rejects_output_beyond_the_protocol_limit() {
        let mut output = LimitedWriter::new(Vec::new(), 3);
        assert_eq!(output.write(b"abc").ok(), Some(3));
        assert!(output.write(b"d").is_err());
    }
}
