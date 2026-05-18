use crate::PlayerRuntimeResult;

use super::types::{DownloadAssetIndex, DownloadTaskId, DownloadTaskSnapshot};

pub trait DownloadExecutor {
    fn prepare(
        &mut self,
        task: &DownloadTaskSnapshot,
    ) -> PlayerRuntimeResult<DownloadPrepareResult>;

    fn start(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()>;

    fn pause(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()>;

    fn resume(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()>;

    fn remove(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadPrepareResult {
    Ready(Option<DownloadAssetIndex>),
    Pending,
}

#[derive(Debug, Default)]
pub struct InMemoryDownloadExecutor {
    prepared: Vec<DownloadTaskId>,
    started: Vec<DownloadTaskId>,
    paused: Vec<DownloadTaskId>,
    resumed: Vec<DownloadTaskId>,
    removed: Vec<DownloadTaskId>,
}

impl InMemoryDownloadExecutor {
    pub fn prepared(&self) -> &[DownloadTaskId] {
        &self.prepared
    }

    pub fn started(&self) -> &[DownloadTaskId] {
        &self.started
    }

    pub fn paused(&self) -> &[DownloadTaskId] {
        &self.paused
    }

    pub fn resumed(&self) -> &[DownloadTaskId] {
        &self.resumed
    }

    pub fn removed(&self) -> &[DownloadTaskId] {
        &self.removed
    }
}

impl DownloadExecutor for InMemoryDownloadExecutor {
    fn prepare(
        &mut self,
        task: &DownloadTaskSnapshot,
    ) -> PlayerRuntimeResult<DownloadPrepareResult> {
        self.prepared.push(task.task_id);
        Ok(DownloadPrepareResult::Ready(None))
    }

    fn start(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.started.push(task.task_id);
        Ok(())
    }

    fn pause(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()> {
        self.paused.push(task_id);
        Ok(())
    }

    fn resume(&mut self, task: &DownloadTaskSnapshot) -> PlayerRuntimeResult<()> {
        self.resumed.push(task.task_id);
        Ok(())
    }

    fn remove(&mut self, task_id: DownloadTaskId) -> PlayerRuntimeResult<()> {
        self.removed.push(task_id);
        Ok(())
    }
}
