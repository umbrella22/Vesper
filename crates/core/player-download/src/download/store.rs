use std::collections::HashMap;

use crate::PlayerResult;

use super::types::{DownloadAssetId, DownloadTaskId, DownloadTaskSnapshot};

pub trait DownloadStore {
    fn save_task(&mut self, task: DownloadTaskSnapshot) -> PlayerResult<()>;

    fn task(&self, task_id: DownloadTaskId) -> Option<DownloadTaskSnapshot>;

    fn tasks(&self) -> Vec<DownloadTaskSnapshot>;

    fn tasks_for_asset(&self, asset_id: &DownloadAssetId) -> Vec<DownloadTaskSnapshot>;
}

#[derive(Debug, Default)]
pub struct InMemoryDownloadStore {
    tasks: HashMap<DownloadTaskId, DownloadTaskSnapshot>,
    asset_index: HashMap<DownloadAssetId, Vec<DownloadTaskId>>,
}

impl DownloadStore for InMemoryDownloadStore {
    fn save_task(&mut self, task: DownloadTaskSnapshot) -> PlayerResult<()> {
        let task_id = task.task_id;
        let asset_id = task.asset_id.clone();

        if let Some(previous) = self.tasks.insert(task_id, task.clone())
            && previous.asset_id != asset_id
        {
            self.remove_from_asset_index(&previous.asset_id, task_id);
        }

        let entry = self.asset_index.entry(asset_id).or_default();
        if !entry.contains(&task_id) {
            entry.push(task_id);
        }
        entry.sort_by_key(|task_id| task_id.get());

        Ok(())
    }

    fn task(&self, task_id: DownloadTaskId) -> Option<DownloadTaskSnapshot> {
        self.tasks.get(&task_id).cloned()
    }

    fn tasks(&self) -> Vec<DownloadTaskSnapshot> {
        let mut tasks = self.tasks.values().cloned().collect::<Vec<_>>();
        tasks.sort_by_key(|task| task.task_id.get());
        tasks
    }

    fn tasks_for_asset(&self, asset_id: &DownloadAssetId) -> Vec<DownloadTaskSnapshot> {
        let mut tasks = self
            .asset_index
            .get(asset_id)
            .into_iter()
            .flat_map(|task_ids| task_ids.iter())
            .filter_map(|task_id| self.tasks.get(task_id))
            .cloned()
            .collect::<Vec<_>>();
        tasks.sort_by_key(|task| task.task_id.get());
        tasks
    }
}

impl InMemoryDownloadStore {
    fn remove_from_asset_index(&mut self, asset_id: &DownloadAssetId, task_id: DownloadTaskId) {
        if let Some(task_ids) = self.asset_index.get_mut(asset_id) {
            task_ids.retain(|existing| *existing != task_id);
            if task_ids.is_empty() {
                self.asset_index.remove(asset_id);
            }
        }
    }
}
