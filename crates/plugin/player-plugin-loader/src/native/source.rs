use super::*;
use serde::de::DeserializeOwned;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use player_plugin::{
    SourceNormalizerPacketSessionConfig, SourceNormalizerPacketStreamInfo,
    SourceNormalizerResourceSessionConfig, SourceNormalizerResourceSessionStatus,
    validate_source_normalizer_plugin_input,
};

const MAX_PACKET_BYTES: usize = 64 * 1024 * 1024;

impl NativePlugin {
    pub(super) fn packet_capabilities<T: DeserializeOwned>(&self) -> PluginResult<T> {
        let raw = self.query(&VESPER_SOURCE_NORMALIZER_ID)?.cast();
        // SAFETY: the UUID identifies the packet interface and its nullable suffix.
        let table: VesperSourceNormalizerVtable = unsafe {
            read_vtable(
                raw,
                offset_of!(VesperSourceNormalizerVtable, get_last_error),
            )
        }?;
        self.read_source_capabilities(table.query_capabilities_json)
    }

    pub(super) fn resource_capabilities<T: DeserializeOwned>(&self) -> PluginResult<T> {
        let raw = self.query(&VESPER_SOURCE_NORMALIZER_RESOURCE_ID)?.cast();
        // SAFETY: the UUID identifies the resource interface and its nullable suffix.
        let table: VesperSourceNormalizerResourceVtable = unsafe {
            read_vtable(
                raw,
                offset_of!(VesperSourceNormalizerResourceVtable, get_last_error),
            )
        }?;
        self.read_source_capabilities(table.query_capabilities_json)
    }

    fn read_source_capabilities<T: DeserializeOwned>(
        &self,
        query: VesperGetJson,
    ) -> PluginResult<T> {
        let query = query.ok_or(PluginError::Contract("missing source capability query"))?;
        let mut out = VesperBytes::default();
        // SAFETY: the queried root is live and out is writable.
        check_status(unsafe { query(self.root.0.owner, &mut out) })?;
        // SAFETY: the root retains the immutable result throughout this call.
        serde_json::from_slice(unsafe { read_bytes(out, MAX_JSON_BYTES) }?).map_err(payload_error)
    }

    pub fn open_packet_stream(
        &self,
        config: &SourceNormalizerPacketSessionConfig,
    ) -> PluginResult<NativePacketStream> {
        validate_source_normalizer_plugin_input(&config.input, &config.headers)
            .map_err(|_| PluginError::Contract("source locator may not enter a plugin"))?;
        let table = self
            .query(&VESPER_SOURCE_NORMALIZER_ID)?
            .cast::<VesperSourceNormalizerVtable>();
        // SAFETY: the interface UUID identifies this table and optional suffix.
        let table = unsafe {
            read_vtable(
                table,
                offset_of!(VesperSourceNormalizerVtable, get_last_error),
            )
        }?;
        let (Some(open), Some(destroy), Some(get_info), Some(_), Some(_), Some(_), Some(_)) = (
            table.open,
            table.destroy,
            table.get_stream_info,
            table.read_packet,
            table.release_packet,
            table.seek,
            table.flush,
        ) else {
            return Err(PluginError::Contract("incomplete packet stream table"));
        };
        let params = serde_json::to_vec(config).map_err(payload_error)?;
        let session = SourceSession::open(
            Arc::clone(&self.root),
            open,
            destroy,
            &config.input,
            &params,
        )?;
        let info = session.json(get_info)?;
        Ok(NativePacketStream {
            info,
            owner: Arc::new(PacketOwner {
                session,
                table,
                leased: AtomicBool::new(false),
                poisoned: AtomicBool::new(false),
            }),
        })
    }

    pub fn open_source_resource(
        &self,
        config: &SourceNormalizerResourceSessionConfig,
    ) -> PluginResult<NativeSourceResource> {
        validate_source_normalizer_plugin_input(&config.input, &config.headers)
            .map_err(|_| PluginError::Contract("source locator may not enter a plugin"))?;
        let table = self
            .query(&VESPER_SOURCE_NORMALIZER_RESOURCE_ID)?
            .cast::<VesperSourceNormalizerResourceVtable>();
        // SAFETY: the interface UUID identifies this table and optional suffix.
        let table = unsafe {
            read_vtable(
                table,
                offset_of!(VesperSourceNormalizerResourceVtable, get_last_error),
            )
        }?;
        let (Some(open), Some(destroy), Some(get_status)) =
            (table.open, table.destroy, table.get_resource_json)
        else {
            return Err(PluginError::Contract(
                "incomplete resource normalizer table",
            ));
        };
        let params = serde_json::to_vec(config).map_err(payload_error)?;
        let session = SourceSession::open(
            Arc::clone(&self.root),
            open,
            destroy,
            &config.input,
            &params,
        )?;
        Ok(NativeSourceResource {
            session,
            get_status,
            wait: table.wait_for_update,
            cancel: table.cancel,
        })
    }
}

fn payload_error(_: serde_json::Error) -> PluginError {
    PluginError::Contract("invalid normalizer JSON payload")
}

type Open = unsafe extern "C" fn(
    *mut c_void,
    VesperBytes,
    VesperBytes,
    *mut *mut VesperSession,
) -> VesperStatus;
type Destroy = unsafe extern "C" fn(*mut VesperSession);
type GetJson = unsafe extern "C" fn(*mut VesperSession, *mut VesperBytes) -> VesperStatus;

struct SourceSession {
    raw: NonNull<VesperSession>,
    destroy: Destroy,
    _root: Arc<Root>,
}

// SAFETY: the ABI permits exclusive source-session access to move across threads.
unsafe impl Send for SourceSession {}

impl SourceSession {
    fn open(
        root: Arc<Root>,
        open: Open,
        destroy: Destroy,
        uri: &str,
        params: &[u8],
    ) -> PluginResult<Self> {
        let mut raw = ptr::null_mut();
        // SAFETY: the owner is live and the two byte views are borrowed through the call only.
        let status = unsafe { open(root.0.owner, bytes(uri.as_bytes()), bytes(params), &mut raw) };
        let session = NonNull::new(raw).map(|raw| Self {
            raw,
            destroy,
            _root: root,
        });
        check_status(status)?;
        session.ok_or(PluginError::Contract("source open returned a null session"))
    }

    fn json<T: DeserializeOwned>(&self, read: GetJson) -> PluginResult<T> {
        let mut out = VesperBytes::default();
        // SAFETY: the caller serializes session callbacks and owns the writable result.
        check_status(unsafe { read(self.raw.as_ptr(), &mut out) })?;
        // SAFETY: output remains live until the next call on this session.
        let json = unsafe { read_bytes(out, MAX_JSON_BYTES) }?;
        serde_json::from_slice(json).map_err(payload_error)
    }
}

impl Drop for SourceSession {
    fn drop(&mut self) {
        // SAFETY: this is the unique owner of the successful open output.
        unsafe { (self.destroy)(self.raw.as_ptr()) };
    }
}

/// Retains the resource and its producer while the platform player consumes it.
pub struct NativeSourceResource {
    session: SourceSession,
    get_status: GetJson,
    wait: Option<unsafe extern "C" fn(*mut VesperSession, u32) -> VesperStatus>,
    cancel: Option<unsafe extern "C" fn(*mut VesperSession) -> VesperStatus>,
}

impl NativeSourceResource {
    pub fn poll(&mut self) -> PluginResult<SourceNormalizerResourceSessionStatus> {
        self.session.json(self.get_status)
    }

    /// Returns false if the producer did not publish an update before the deadline.
    pub fn wait_for_update(&mut self, timeout: Duration) -> PluginResult<bool> {
        let wait = self
            .wait
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        let timeout_ms = u32::try_from(timeout.as_millis())
            .map_err(|_| PluginError::Contract("resource wait exceeds the ABI timeout range"))?;
        // SAFETY: the resource session is live and exclusively owned for this call.
        status_or_pending(unsafe { wait(self.session.raw.as_ptr(), timeout_ms) })
    }

    /// Requests cancellation. False indicates the producer is still stopping.
    pub fn cancel(&mut self) -> PluginResult<bool> {
        let cancel = self
            .cancel
            .ok_or(PluginError::Status(VESPER_ERR_UNSUPPORTED))?;
        // SAFETY: the resource session is live and exclusively owned for this call.
        status_or_pending(unsafe { cancel(self.session.raw.as_ptr()) })
    }

    pub(super) fn supports_resource_control(&self) -> bool {
        self.wait.is_some() && self.cancel.is_some()
    }
}

fn status_or_pending(status: VesperStatus) -> PluginResult<bool> {
    match status {
        VESPER_OK => Ok(true),
        VESPER_TRY_AGAIN => Ok(false),
        status => Err(PluginError::Status(status)),
    }
}

struct PacketOwner {
    session: SourceSession,
    table: VesperSourceNormalizerVtable,
    leased: AtomicBool,
    poisoned: AtomicBool,
}

// SAFETY: while a packet is live all stream methods refuse calls. Its unique
// lease can run release on another thread before clearing the leased flag.
unsafe impl Sync for PacketOwner {}

pub struct NativePacketStream {
    info: SourceNormalizerPacketStreamInfo,
    owner: Arc<PacketOwner>,
}

pub enum NativePacketStreamPoll {
    Packet(NativeNormalizedPacket),
    Pending,
    EndOfStream,
}

/// Owns the borrow of one packet. Dropping the stream first keeps its bytes live.
pub struct NativeNormalizedPacket {
    descriptor: VesperPacket,
    lease: u64,
    owner: Arc<PacketOwner>,
}

// SAFETY: the bytes are immutable until release and release is exclusive.
unsafe impl Send for NativeNormalizedPacket {}

impl NativeNormalizedPacket {
    pub fn descriptor(&self) -> &VesperPacket {
        &self.descriptor
    }
}

impl AsRef<[u8]> for NativeNormalizedPacket {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: successful reads check the data pointer and hold the lease until Drop.
        unsafe { std::slice::from_raw_parts(self.descriptor.data, self.descriptor.len as usize) }
    }
}

impl Drop for NativeNormalizedPacket {
    fn drop(&mut self) {
        self.owner.release(self.lease);
    }
}

impl PacketOwner {
    fn release(&self, id: u64) {
        if id != 0
            && let Some(release) = self.table.release_packet
        {
            // SAFETY: this is the unique outstanding lease. No session callback
            // can run until leased is cleared below.
            if unsafe { release(self.session.raw.as_ptr(), id) } != VESPER_OK {
                self.poisoned.store(true, Ordering::Release);
            }
        }
        self.leased.store(false, Ordering::Release);
    }
}

impl NativePacketStream {
    pub fn stream_info(&self) -> &SourceNormalizerPacketStreamInfo {
        &self.info
    }

    pub fn read(&mut self) -> PluginResult<NativePacketStreamPoll> {
        if self.owner.leased.load(Ordering::Acquire) {
            return Ok(NativePacketStreamPoll::Pending);
        }
        self.check_live()?;
        let read = self
            .owner
            .table
            .read_packet
            .ok_or(PluginError::Contract("missing read_packet"))?;
        self.owner.leased.store(true, Ordering::Release);
        let mut descriptor = VesperPacket {
            struct_size: size_of::<VesperPacket>() as u32,
            ..Default::default()
        };
        let mut id = 0;
        // SAFETY: reader has exclusive access and passes writable ABI outputs.
        let status = unsafe { read(self.owner.session.raw.as_ptr(), &mut descriptor, &mut id) };
        if status == VESPER_OK
            && id != 0
            && descriptor.struct_size >= size_of::<VesperPacket>() as u32
            && descriptor.len > 0
            && descriptor.len <= MAX_PACKET_BYTES as u64
            && !descriptor.data.is_null()
        {
            return Ok(NativePacketStreamPoll::Packet(NativeNormalizedPacket {
                descriptor,
                lease: id,
                owner: Arc::clone(&self.owner),
            }));
        }
        self.owner.release(id);
        if status == VESPER_ERR_PANIC || status == VESPER_OK || id != 0 {
            self.owner.poisoned.store(true, Ordering::Release);
        }
        match status {
            VESPER_TRY_AGAIN | VESPER_NEED_MORE_INPUT if id == 0 => {
                Ok(NativePacketStreamPoll::Pending)
            }
            VESPER_EOF if id == 0 => Ok(NativePacketStreamPoll::EndOfStream),
            VESPER_OK => Err(PluginError::Contract("invalid normalizer packet lease")),
            value => Err(PluginError::Status(value)),
        }
    }

    /// Seek backward to a decodable point for a position measured from media start.
    pub fn seek(&mut self, position: Duration) -> PluginResult<()> {
        if self.owner.leased.load(Ordering::Acquire) {
            return Err(PluginError::Status(VESPER_TRY_AGAIN));
        }
        self.check_live()?;
        let seek = self
            .owner
            .table
            .seek
            .ok_or(PluginError::Contract("missing packet seek"))?;
        let pts = i64::try_from(position.as_micros())
            .map_err(|_| PluginError::Contract("invalid packet seek timestamp"))?;
        // SAFETY: no packet borrow is outstanding and calls are serialized.
        let status = unsafe { seek(self.owner.session.raw.as_ptr(), pts, VESPER_SEEK_BACKWARD) };
        if status == VESPER_ERR_PANIC {
            self.owner.poisoned.store(true, Ordering::Release);
        }
        check_status(status)
    }

    pub fn flush(&mut self) -> PluginResult<()> {
        if self.owner.leased.load(Ordering::Acquire) {
            return Err(PluginError::Status(VESPER_TRY_AGAIN));
        }
        self.check_live()?;
        let flush = self
            .owner
            .table
            .flush
            .ok_or(PluginError::Contract("missing packet flush"))?;
        // SAFETY: no packet borrow is outstanding and calls are serialized.
        let status = unsafe { flush(self.owner.session.raw.as_ptr()) };
        if status == VESPER_ERR_PANIC {
            self.owner.poisoned.store(true, Ordering::Release);
        }
        check_status(status)
    }

    fn check_live(&self) -> PluginResult<()> {
        if self.owner.poisoned.load(Ordering::Acquire) {
            return Err(PluginError::Status(VESPER_ERR_ABORTED));
        }
        Ok(())
    }
}
