use std::time::Duration;

use tokio::sync::mpsc;
use tracing::debug;

use crate::{MediaSource, PlaybackState, PlayerError};

const DEFAULT_CHANNEL_CAPACITY: usize = 32;

#[derive(Debug, Clone)]
pub struct PlayerConfig {
    pub enable_audio: bool,
    pub enable_video: bool,
    pub buffered_packet_limit: usize,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            enable_audio: true,
            enable_video: true,
            buffered_packet_limit: 128,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PlaybackCommand {
    Load(MediaSource),
    Play,
    Pause,
    Stop,
    Seek(Duration),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    StateChanged(PlaybackState),
    SourceLoaded(MediaSource),
    SeekCompleted(Duration),
    Shutdown,
}

#[derive(Debug)]
pub struct Player {
    config: PlayerConfig,
    state: PlaybackState,
    command_rx: mpsc::Receiver<PlaybackCommand>,
    event_tx: mpsc::Sender<PlayerEvent>,
}

#[derive(Clone, Debug)]
pub struct PlayerHandle {
    command_tx: mpsc::Sender<PlaybackCommand>,
}

impl PlayerHandle {
    pub async fn load(&self, source: MediaSource) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Load(source)).await
    }

    pub async fn play(&self) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Play).await
    }

    pub async fn pause(&self) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Pause).await
    }

    pub async fn stop(&self) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Stop).await
    }

    pub async fn seek(&self, position: Duration) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Seek(position)).await
    }

    pub async fn shutdown(&self) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Shutdown).await
    }

    async fn send(&self, command: PlaybackCommand) -> Result<(), PlayerError> {
        self.command_tx
            .send(command)
            .await
            .map_err(|_| PlayerError::CommandChannelClosed)
    }
}

impl Player {
    pub fn new(config: PlayerConfig) -> (Self, PlayerHandle, mpsc::Receiver<PlayerEvent>) {
        let (command_tx, command_rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel(DEFAULT_CHANNEL_CAPACITY);

        (
            Self {
                config,
                state: PlaybackState::Idle,
                command_rx,
                event_tx,
            },
            PlayerHandle { command_tx },
            event_rx,
        )
    }

    pub fn config(&self) -> &PlayerConfig {
        &self.config
    }

    pub async fn run(mut self) -> Result<(), PlayerError> {
        self.publish_state().await?;

        while let Some(command) = self.command_rx.recv().await {
            let keep_running = self.handle_command(command).await?;
            if !keep_running {
                break;
            }
        }

        Ok(())
    }

    async fn handle_command(&mut self, command: PlaybackCommand) -> Result<bool, PlayerError> {
        match command {
            PlaybackCommand::Load(source) => {
                self.set_state(PlaybackState::Loading).await?;
                self.event_tx
                    .send(PlayerEvent::SourceLoaded(source))
                    .await
                    .map_err(|_| PlayerError::EventChannelClosed)?;
                self.set_state(PlaybackState::Ready).await?;
                Ok(true)
            }
            PlaybackCommand::Play => {
                self.set_state(PlaybackState::Playing).await?;
                Ok(true)
            }
            PlaybackCommand::Pause => {
                self.set_state(PlaybackState::Paused).await?;
                Ok(true)
            }
            PlaybackCommand::Stop => {
                self.set_state(PlaybackState::Stopped).await?;
                Ok(true)
            }
            PlaybackCommand::Seek(position) => {
                self.event_tx
                    .send(PlayerEvent::SeekCompleted(position))
                    .await
                    .map_err(|_| PlayerError::EventChannelClosed)?;
                Ok(true)
            }
            PlaybackCommand::Shutdown => {
                self.event_tx
                    .send(PlayerEvent::Shutdown)
                    .await
                    .map_err(|_| PlayerError::EventChannelClosed)?;
                Ok(false)
            }
        }
    }

    async fn set_state(&mut self, state: PlaybackState) -> Result<(), PlayerError> {
        self.state = state;
        self.publish_state().await
    }

    async fn publish_state(&self) -> Result<(), PlayerError> {
        debug!(state = ?self.state, "player state transition");
        self.event_tx
            .send(PlayerEvent::StateChanged(self.state.clone()))
            .await
            .map_err(|_| PlayerError::EventChannelClosed)
    }
}
