use crate::{failure, success};
use player_download::{
    DownloadAssetId, DownloadAssetIndex, DownloadContentFormat, DownloadManagerConfig,
    DownloadProfile, DownloadProgressSnapshot, DownloadSource, DownloadTaskId,
    DownloadTaskSnapshot, DownloadTaskStatus,
};
use player_download::{
    DownloadPlanner, DownloadPlanningClient, PlayerError, PlayerErrorCategory, PlayerErrorCode,
    PlayerResult,
};
use player_model::MediaSource;
use player_platform_mobile::{MobileDownloadBridgeSession, MobileDownloadCommand};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    path::{Component, PathBuf},
    sync::Arc,
    time::Instant,
};

type Result<T> = std::result::Result<T, Value>;
// Fits a two-hour, six-second, separate audio/video VOD within the existing
// 1 MiB bridge and 4 MiB checkpoint budgets. Byte budgets remain independent.
const MAX_OUTPUT_FILES: usize = 4096;
const MAX_DISCOVERY_BATCH: usize = 128;
fn invalid() -> Value {
    failure("invalid_argument", "download command has an invalid field")
}
fn number(value: &Value, field: &str) -> Result<u64> {
    let text = value[field].as_str().ok_or_else(invalid)?;
    if text.is_empty() || text.len() > 20 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    text.parse().map_err(|_| invalid())
}
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .filter(|text| {
            !text.is_empty() && text.len() <= 8192 && !text.chars().any(char::is_control)
        })
        .ok_or_else(invalid)
}
fn operation<T>(result: PlayerResult<T>) -> Result<T> {
    result.map_err(|_| failure("download_failed", "shared download operation was rejected"))
}

#[derive(Deserialize)]
struct TextRecord {
    uri: String,
    text: String,
}
#[derive(Deserialize)]
struct LengthRecord {
    uri: String,
    length: String,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Inventory {
    texts: Vec<TextRecord>,
    lengths: Vec<LengthRecord>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct ProfileWire {
    variant_id: Option<String>,
    preferred_audio_language: Option<String>,
}
struct InventoryClient {
    texts: HashMap<String, String>,
    lengths: HashMap<String, u64>,
    missing: RefCell<Vec<Value>>,
}
impl DownloadPlanningClient for InventoryClient {
    fn fetch_text(&self, uri: &str) -> PlayerResult<String> {
        if let Some(text) = self.texts.get(uri) {
            return Ok(text.clone());
        }
        self.missing
            .borrow_mut()
            .push(json!({"kind":"text","uri":uri}));
        Err(PlayerError::with_category(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            "planning inventory is incomplete",
        ))
    }
    fn content_length(&self, uri: &str) -> PlayerResult<Option<u64>> {
        if let Some(length) = self.lengths.get(uri) {
            return Ok(Some(*length));
        }
        self.missing
            .borrow_mut()
            .push(json!({"kind":"length","uri":uri}));
        Err(PlayerError::with_category(
            PlayerErrorCode::InvalidSource,
            PlayerErrorCategory::Source,
            "planning inventory is incomplete",
        ))
    }
    fn content_lengths(&self, uris: &[&str]) -> PlayerResult<Vec<Option<u64>>> {
        let mut missing = self.missing.borrow_mut();
        let mut seen = HashSet::new();
        for uri in uris {
            if !self.lengths.contains_key(*uri)
                && seen.insert(*uri)
                && missing.len() < MAX_DISCOVERY_BATCH
            {
                missing.push(json!({"kind":"length","uri":uri}));
            }
        }
        if !missing.is_empty() {
            return Err(PlayerError::with_category(
                PlayerErrorCode::InvalidSource,
                PlayerErrorCategory::Source,
                "planning inventory is incomplete",
            ));
        }
        Ok(uris
            .iter()
            .map(|uri| self.lengths.get(*uri).copied())
            .collect())
    }
}
struct Plan {
    source: DownloadSource,
    profile: DownloadProfile,
    index: DownloadAssetIndex,
    bytes: usize,
}

/// CPU-only planner and shared task manager. Host I/O carries a fenced attempt ID.
pub struct Downloads {
    bridge: MobileDownloadBridgeSession,
    plans: BTreeMap<u64, Plan>,
    task_plans: BTreeMap<u64, u64>,
    attempts: BTreeMap<u64, u64>,
    next_plan: u64,
    next_attempt: u64,
    maximum: usize,
}
impl Downloads {
    pub(crate) fn new(config: &Value) -> Result<Self> {
        let maximum = config
            .get("maxTasks")
            .map_or(Some(32), Value::as_u64)
            .filter(|limit| (1..=64).contains(limit))
            .ok_or_else(invalid)? as usize;
        Ok(Self {
            bridge: MobileDownloadBridgeSession::new(DownloadManagerConfig::default(), "harmony"),
            plans: BTreeMap::new(),
            task_plans: BTreeMap::new(),
            attempts: BTreeMap::new(),
            next_plan: 1,
            next_attempt: 1,
            maximum,
        })
    }
    pub(crate) fn execute(&mut self, value: &Value) -> Value {
        match self.try_execute(value) {
            Ok(result) => success(result),
            Err(error) => error,
        }
    }
    fn plan(&mut self, value: &Value) -> Result<Value> {
        if self.plans.len() >= self.maximum {
            return Err(failure("capacity", "download plan capacity reached"));
        }
        let source_value = value.get("source").ok_or_else(invalid)?;
        crate::sources::validate_descriptor(source_value)?;
        if source_value["kind"] != "remote" {
            return Err(failure(
                "unsupported",
                "downloads require an accepted remote source",
            ));
        }
        let format = match source_value["protocol"].as_str() {
            Some("progressive") => DownloadContentFormat::SingleFile,
            Some("hls") => DownloadContentFormat::HlsSegments,
            Some("dash") => DownloadContentFormat::DashSegments,
            _ => {
                return Err(failure(
                    "unsupported",
                    "download protocol is outside the finite profile",
                ));
            }
        };
        let source = DownloadSource::new(MediaSource::new(text(source_value, "uri")?), format);
        let mut profile = DownloadProfile::default();
        let profile_wire: ProfileWire =
            serde_json::from_value(value.get("profile").cloned().unwrap_or_else(|| json!({})))
                .map_err(|_| {
                    failure(
                        "unsupported",
                        "download profile contains unknown or invalid fields",
                    )
                })?;
        if profile_wire
            .variant_id
            .as_ref()
            .is_some_and(|value| value.len() > 512 || value.chars().any(char::is_control))
            || profile_wire
                .preferred_audio_language
                .as_ref()
                .is_some_and(|value| value.len() > 64 || value.chars().any(char::is_control))
        {
            return Err(invalid());
        }
        profile.variant_id = profile_wire.variant_id;
        profile.preferred_audio_language = profile_wire.preferred_audio_language;
        let inventory: Inventory =
            serde_json::from_value(value.get("inventory").cloned().unwrap_or_else(|| json!({})))
                .map_err(|_| invalid())?;
        if inventory.texts.len() > 128 || inventory.lengths.len() > MAX_OUTPUT_FILES {
            return Err(failure("capacity", "planning inventory capacity reached"));
        }
        // Protected MPDs are rejected by the same XML-aware startup boundary.
        if format == DownloadContentFormat::DashSegments {
            for record in &inventory.texts {
                // Timeline expansion and local numbering use the shared planner.
                // SegmentList has no offline mapping in this profile.
                let mut reader = quick_xml::Reader::from_str(&record.text);
                loop {
                    match reader.read_event() {
                        Ok(
                            quick_xml::events::Event::Start(tag)
                            | quick_xml::events::Event::Empty(tag),
                        ) => {
                            if matches!(tag.local_name().as_ref(), "SegmentList") {
                                return Err(failure(
                                    "unsupported",
                                    "DASH downloads do not admit SegmentList",
                                ));
                            }
                        }
                        Ok(quick_xml::events::Event::Eof) => break,
                        Err(_) => return Err(invalid()),
                        _ => {}
                    }
                }
                let request = json!({"operation":"parse_manifest","mpd":record.text,"manifestUrl":record.uri});
                let response = crate::wire::dash(&request.to_string(), &request);
                if serde_json::from_str::<Value>(&response)
                    .ok()
                    .is_none_or(|response| response["ok"] != true)
                {
                    return Err(failure(
                        "unsupported",
                        "DASH download is outside the unprotected finite profile",
                    ));
                }
            }
        }
        let client = InventoryClient {
            texts: inventory
                .texts
                .into_iter()
                .map(|record| (record.uri, record.text))
                .collect(),
            lengths: inventory
                .lengths
                .into_iter()
                .map(|record| {
                    record
                        .length
                        .parse::<u64>()
                        .map(|length| (record.uri, length))
                })
                .collect::<std::result::Result<_, _>>()
                .map_err(|_| invalid())?,
            missing: RefCell::new(Vec::new()),
        };
        let planner = DownloadPlanner::new(client).with_maximum_files(MAX_OUTPUT_FILES);
        let planned = planner.plan(&source, &profile);
        if !planner.client().missing.borrow().is_empty() {
            return Ok(json!({"needs": *planner.client().missing.borrow()}));
        }
        let index = planned.map_err(|_| {
            failure(
                "unsupported",
                "media is outside the supported finite download profile",
            )
        })?;
        let wire = index_value(&index)?;
        let bytes = wire.to_string().len()
            + source.source.uri().len()
            + profile.variant_id.as_ref().map_or(0, String::len)
            + profile
                .preferred_audio_language
                .as_ref()
                .map_or(0, String::len);
        if bytes > 900 * 1024
            || self.plans.values().map(|plan| plan.bytes).sum::<usize>() + bytes > 4 * 1024 * 1024
        {
            return Err(failure("capacity", "download plan byte budget reached"));
        }
        let id = self.next_plan;
        self.next_plan = id.checked_add(1).ok_or_else(invalid)?;
        self.plans.insert(
            id,
            Plan {
                source,
                profile,
                index,
                bytes,
            },
        );
        Ok(json!({"planId":id.to_string(),"index":wire,"needs":[]}))
    }
    fn try_execute(&mut self, value: &Value) -> Result<Value> {
        let now = Instant::now();
        match value["op"].as_str() {
            Some("plan") => self.plan(value),
            Some("discardPlan") => {
                let plan = number(value, "planId")?;
                if self.task_plans.values().any(|id| *id == plan) {
                    return Err(failure("in_use", "download plan is retained by a task"));
                }
                self.plans.remove(&plan);
                Ok(json!({}))
            }
            Some("create") => {
                if self.task_plans.len() >= self.maximum {
                    return Err(failure(
                        "capacity",
                        "retained download task capacity reached",
                    ));
                }
                let plan_id = number(value, "planId")?;
                let plan = self.plans.get(&plan_id).ok_or_else(invalid)?;
                let asset_id = text(value, "assetId")?;
                if asset_id.len() > 256 || asset_id.trim() != asset_id {
                    return Err(invalid());
                }
                let task = operation(self.bridge.create_task(
                    asset_id,
                    plan.source.clone(),
                    plan.profile.clone(),
                    plan.index.clone(),
                    now,
                ))?;
                self.task_plans.insert(task.get(), plan_id);
                self.response(task)
            }
            Some("snapshot") => Ok(
                json!({"tasks":self.bridge.snapshot().tasks.iter().map(snapshot).collect::<Vec<_>>()}),
            ),
            Some("restore") => {
                if !self.task_plans.is_empty() {
                    return Err(failure(
                        "invalid_state",
                        "restore requires a new download session",
                    ));
                }
                let records = value["tasks"]
                    .as_array()
                    .filter(|records| records.len() <= self.maximum)
                    .ok_or_else(invalid)?;
                let mut tasks = Vec::new();
                let mut ids = HashSet::new();
                let mut removed = HashSet::new();
                for record in records {
                    let id = number(record, "taskId")?;
                    if id == 0 || !ids.insert(id) {
                        return Err(invalid());
                    }
                    let asset = text(record, "assetId")?;
                    if asset.len() > 256 || asset.trim() != asset {
                        return Err(invalid());
                    }
                    let total = record
                        .get("totalBytes")
                        .filter(|value| !value.is_null())
                        .map(|_| number(record, "totalBytes"))
                        .transpose()?;
                    if record["removed"] == true {
                        removed.insert(id);
                    }
                    let status = if record["verifiedComplete"] == true {
                        DownloadTaskStatus::Completed
                    } else {
                        DownloadTaskStatus::Paused
                    };
                    tasks.push(DownloadTaskSnapshot {
                        task_id: DownloadTaskId::from_raw(id),
                        asset_id: DownloadAssetId::new(asset),
                        source: DownloadSource::new(
                            MediaSource::new("vesper-pending-access://restored"),
                            DownloadContentFormat::Unknown,
                        ),
                        profile: DownloadProfile::default(),
                        status,
                        progress: DownloadProgressSnapshot {
                            received_bytes: number(record, "receivedBytes")?,
                            total_bytes: total,
                            received_segments: 0,
                            total_segments: None,
                        },
                        asset_index: Arc::new(DownloadAssetIndex {
                            total_size_bytes: total,
                            ..DownloadAssetIndex::default()
                        }),
                        created_at: now,
                        updated_at: now,
                        error_summary: None,
                    });
                }
                let restored = operation(self.bridge.restore_tasks(tasks, now))?;
                for task in &restored {
                    self.task_plans.insert(task.task_id.get(), 0);
                    if removed.contains(&task.task_id.get()) {
                        operation(self.bridge.remove_task(task.task_id, now))?;
                    }
                }
                self.bridge.drain_commands();
                self.bridge.drain_event_batch();
                Ok(
                    json!({"tasks":restored.iter().filter_map(|task| self.bridge.task(task.task_id).as_ref().map(snapshot)).collect::<Vec<_>>()}),
                )
            }
            Some("refresh") => {
                let task = DownloadTaskId::from_raw(number(value, "taskId")?);
                let plan_id = number(value, "planId")?;
                let plan = self.plans.get(&plan_id).ok_or_else(invalid)?;
                let current = self.bridge.task(task).ok_or_else(invalid)?;
                if !matches!(
                    current.status,
                    DownloadTaskStatus::Paused
                        | DownloadTaskStatus::Queued
                        | DownloadTaskStatus::Failed
                ) {
                    return Err(failure(
                        "invalid_state",
                        "refresh requires a stopped download attempt",
                    ));
                }
                self.attempts.remove(&task.get());
                operation(self.bridge.replace_task_plan(
                    task,
                    plan.source.clone(),
                    plan.profile.clone(),
                    plan.index.clone(),
                    now,
                ))?;
                operation(self.bridge.pause_task(task, now))?;
                self.task_plans.insert(task.get(), plan_id);
                self.response(task)
            }
            Some("start" | "resume") => {
                let task = DownloadTaskId::from_raw(number(value, "taskId")?);
                let current = self.bridge.task(task).ok_or_else(invalid)?;
                if self.task_plans.get(&task.get()) == Some(&0) {
                    return Err(failure(
                        "access_context_required",
                        "restore requires a refreshed source before network execution",
                    ));
                }
                if (value["op"] == "start" && current.status != DownloadTaskStatus::Queued)
                    || (value["op"] == "resume" && current.status != DownloadTaskStatus::Paused)
                {
                    return Err(failure(
                        "invalid_state",
                        "download cannot start from its current state",
                    ));
                }
                let attempt = self.next_attempt;
                self.next_attempt = attempt.checked_add(1).ok_or_else(invalid)?;
                if value["op"] == "resume" {
                    operation(self.bridge.resume_task(task, now))?;
                } else {
                    operation(self.bridge.start_task(task, now))?;
                }
                self.attempts.insert(task.get(), attempt);
                self.response(task)
            }
            Some("pause" | "remove") => {
                let task = DownloadTaskId::from_raw(number(value, "taskId")?);
                self.attempts.remove(&task.get());
                if value["op"] == "pause" {
                    operation(self.bridge.pause_task(task, now))?;
                } else {
                    operation(self.bridge.remove_task(task, now))?;
                }
                self.response(task)
            }
            Some("prepared" | "progress" | "complete" | "fail") => {
                let task = DownloadTaskId::from_raw(number(value, "taskId")?);
                let attempt = number(value, "attemptId")?;
                let current = self.bridge.task(task).ok_or_else(invalid)?;
                if self.attempts.get(&task.get()) != Some(&attempt)
                    || !matches!(
                        current.status,
                        DownloadTaskStatus::Preparing | DownloadTaskStatus::Downloading
                    )
                {
                    return Err(failure(
                        "stale_attempt",
                        "download completion does not own the current attempt",
                    ));
                }
                match value["op"].as_str() {
                    Some("prepared") => {
                        if current.status != DownloadTaskStatus::Preparing {
                            return Err(failure("invalid_state", "download is not preparing"));
                        }
                        let plan = self
                            .task_plans
                            .get(&task.get())
                            .and_then(|id| self.plans.get(id))
                            .ok_or_else(invalid)?;
                        operation(
                            self.bridge
                                .complete_preparation(task, plan.index.clone(), now),
                        )?;
                    }
                    Some("progress") => {
                        if current.status != DownloadTaskStatus::Downloading {
                            return Err(failure(
                                "invalid_state",
                                "progress requires an executing download",
                            ));
                        }
                        operation(
                            self.bridge.update_progress(
                                task,
                                number(value, "receivedBytes")?,
                                value["receivedSegments"]
                                    .as_u64()
                                    .and_then(|value| u32::try_from(value).ok())
                                    .ok_or_else(invalid)?,
                                now,
                            ),
                        )?;
                    }
                    Some("complete") => {
                        if current.status != DownloadTaskStatus::Downloading {
                            return Err(failure("invalid_state", "download is not executing"));
                        }
                        if current
                            .progress
                            .total_bytes
                            .is_none_or(|total| current.progress.received_bytes != total)
                            || current
                                .progress
                                .total_segments
                                .is_some_and(|total| current.progress.received_segments != total)
                        {
                            return Err(failure(
                                "incomplete",
                                "download progress has not reached its planned totals",
                            ));
                        }
                        operation(self.bridge.complete_task(task, None, now))?;
                        self.attempts.remove(&task.get());
                    }
                    _ => {
                        operation(self.bridge.fail_task(
                            task,
                            PlayerError::with_category(
                                PlayerErrorCode::InvalidSource,
                                PlayerErrorCategory::Source,
                                "host download execution failed",
                            ),
                            now,
                        ))?;
                        self.attempts.remove(&task.get());
                    }
                }
                self.response(task)
            }
            _ => Err(failure("unsupported", "unknown download operation")),
        }
    }
    fn response(&mut self, task: DownloadTaskId) -> Result<Value> {
        let snapshot_value = self
            .bridge
            .task(task)
            .as_ref()
            .map(snapshot)
            .ok_or_else(invalid)?;
        let actions = self
            .bridge
            .drain_commands()
            .iter()
            .map(|command| match command {
                MobileDownloadCommand::Prepare { task } => {
                    json!({"type":"prepare","taskId":task.task_id.get().to_string()})
                }
                MobileDownloadCommand::Start { task } => {
                    json!({"type":"start","taskId":task.task_id.get().to_string()})
                }
                MobileDownloadCommand::Resume { task } => {
                    json!({"type":"resume","taskId":task.task_id.get().to_string()})
                }
                MobileDownloadCommand::Remove { task } => {
                    json!({"type":"remove","taskId":task.task_id.get().to_string()})
                }
                MobileDownloadCommand::Pause { task_id } => {
                    json!({"type":"pause","taskId":task_id.get().to_string()})
                }
            })
            .collect::<Vec<_>>();
        // Public snapshots contain no URLs, headers, generated text or source descriptors.
        self.bridge.drain_event_batch();
        Ok(
            json!({"task":snapshot_value,"attemptId":self.attempts.get(&task.get()).map(u64::to_string),"actions":actions}),
        )
    }
}
fn snapshot(task: &DownloadTaskSnapshot) -> Value {
    json!({"taskId":task.task_id.get().to_string(),"assetId":task.asset_id.as_str(),"status":task.status.as_str(),
        "receivedBytes":task.progress.received_bytes.to_string(),"totalBytes":task.progress.total_bytes.map(|value| value.to_string()),
        "receivedSegments":task.progress.received_segments,"totalSegments":task.progress.total_segments})
}
fn index_value(index: &DownloadAssetIndex) -> Result<Value> {
    if index.resources.len() + index.segments.len() > MAX_OUTPUT_FILES {
        return Err(failure(
            "capacity",
            "finite asset exceeds 4096 output files",
        ));
    }
    let mut paths = HashSet::new();
    let validate = |path: &Option<PathBuf>, paths: &mut HashSet<String>| -> Result<String> {
        let path = path.as_ref().ok_or_else(invalid)?;
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(failure(
                "invalid_path",
                "asset output is not a relative file",
            ));
        }
        let value = path
            .to_str()
            .filter(|value| !value.is_empty() && value.len() <= 512 && !value.contains('\\'))
            .ok_or_else(invalid)?
            .to_owned();
        if !paths.insert(value.clone()) {
            return Err(failure("invalid_path", "asset outputs collide"));
        }
        Ok(value)
    };
    let mut files = Vec::new();
    for resource in &index.resources {
        files.push(json!({"id":resource.resource_id,"uri":resource.uri,"path":validate(&resource.relative_path, &mut paths)?,
            "offset":resource.byte_range.map(|range| range.offset.to_string()),"length":resource.byte_range.map(|range| range.length.to_string()),
            "size":resource.size_bytes.map(|size| size.to_string()),"generatedText":resource.generated_text,"segment":false}));
    }
    for segment in &index.segments {
        files.push(json!({"id":segment.segment_id,"uri":segment.uri,"path":validate(&segment.relative_path, &mut paths)?,
            "offset":segment.byte_range.map(|range| range.offset.to_string()),"length":segment.byte_range.map(|range| range.length.to_string()),
            "size":segment.size_bytes.map(|size| size.to_string()),"segment":true}));
    }
    Ok(
        json!({"format":match index.content_format { DownloadContentFormat::SingleFile=>"progressive", DownloadContentFormat::HlsSegments=>"hls", DownloadContentFormat::DashSegments=>"dash", _=>"unknown" },
        "totalBytes":index.inferred_total_size_bytes().map(|bytes| bytes.to_string()),"files":files}),
    )
}
