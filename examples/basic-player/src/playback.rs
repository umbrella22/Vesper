//! Narrow orchestration seam for the example's desktop playback path.
//!
//! This module records ownership boundaries without selecting a decoder or
//! backend. The host still owns runtime commands and frame upload; this type
//! only gives launch cancellation, generation checks, and presentation events
//! one place to reason about stale work.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use player_render_wgpu::RenderFrameOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackLaunchToken {
    pub request_id: u64,
    pub generation: u64,
}

#[derive(Debug)]
pub struct DesktopPlaybackSession {
    next_request_id: u64,
    generation: u64,
    active_request_id: Option<u64>,
    active_cancel: Option<Arc<AtomicBool>>,
    command_count: u64,
    snapshot_count: u64,
    frame_count: u64,
    last_presentation: Option<RenderFrameOutcome>,
}

impl Default for DesktopPlaybackSession {
    fn default() -> Self {
        Self {
            next_request_id: 1,
            generation: 0,
            active_request_id: None,
            active_cancel: None,
            command_count: 0,
            snapshot_count: 0,
            frame_count: 0,
            last_presentation: None,
        }
    }
}

impl DesktopPlaybackSession {
    pub fn begin_source_launch(&mut self) -> (PlaybackLaunchToken, Arc<AtomicBool>) {
        if let Some(cancel) = self.active_cancel.take() {
            cancel.store(true, Ordering::SeqCst);
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.generation = self.generation.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        self.active_request_id = Some(request_id);
        self.active_cancel = Some(cancel.clone());
        (
            PlaybackLaunchToken {
                request_id,
                generation: self.generation,
            },
            cancel,
        )
    }

    pub fn is_current_launch(&self, request_id: u64) -> bool {
        self.active_request_id == Some(request_id)
    }

    pub fn finish_launch(&mut self, request_id: u64) -> bool {
        if !self.is_current_launch(request_id) {
            return false;
        }
        self.active_request_id = None;
        self.active_cancel = None;
        true
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn accept_frame(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.frame_count = self.frame_count.saturating_add(1);
        true
    }

    pub fn record_command_dispatch(&mut self) {
        self.command_count = self.command_count.saturating_add(1);
    }

    pub fn record_snapshot_event(&mut self) {
        self.snapshot_count = self.snapshot_count.saturating_add(1);
    }

    pub fn record_presentation(&mut self, outcome: RenderFrameOutcome) {
        self.last_presentation = Some(outcome);
    }

    #[cfg(test)]
    fn counters(&self) -> (u64, u64, u64) {
        (self.command_count, self.snapshot_count, self.frame_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_launch_cancels_previous_generation() {
        let mut session = DesktopPlaybackSession::default();
        let (first, first_cancel) = session.begin_source_launch();
        let (second, _) = session.begin_source_launch();

        assert!(first_cancel.load(Ordering::SeqCst));
        assert!(!session.is_current_launch(first.request_id));
        assert!(session.is_current_launch(second.request_id));
        assert_ne!(first.generation, second.generation);
        assert!(!session.accept_frame(first.generation));
        assert!(session.accept_frame(second.generation));
    }

    #[test]
    fn boundary_counters_are_explicit_and_bounded() {
        let mut session = DesktopPlaybackSession::default();
        session.record_command_dispatch();
        session.record_snapshot_event();
        let (token, _) = session.begin_source_launch();
        assert!(session.accept_frame(token.generation));
        assert_eq!(session.counters(), (1, 1, 1));
    }
}
