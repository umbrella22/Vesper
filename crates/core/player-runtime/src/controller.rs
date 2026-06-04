use std::time::Duration;

use tokio::sync::mpsc;
use tracing::debug;

use player_model::{MediaSource, PlaybackState, PlayerError};

const DEFAULT_CHANNEL_CAPACITY: usize = 32;

/// Configuration for the lightweight async player command loop.
#[derive(Debug, Clone)]
pub struct PlayerConfig {
    /// Whether audio handling should be enabled by the host.
    pub enable_audio: bool,
    /// Whether video handling should be enabled by the host.
    pub enable_video: bool,
    /// Maximum buffered packet count requested by the host.
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

/// Commands accepted by the lightweight async player loop.
#[derive(Debug, Clone)]
pub enum PlaybackCommand {
    /// Load a new media source.
    Load(MediaSource),
    /// Transition to playing state.
    Play,
    /// Transition to paused state.
    Pause,
    /// Transition to stopped state.
    Stop,
    /// Report a completed seek to the requested position.
    Seek(Duration),
    /// Stop the command loop.
    Shutdown,
}

/// Events emitted by the lightweight async player loop.
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    /// Playback state changed.
    StateChanged(PlaybackState),
    /// A source was accepted by the loop.
    SourceLoaded(MediaSource),
    /// A seek command completed.
    SeekCompleted(Duration),
    /// The loop is shutting down.
    Shutdown,
}

/// Lightweight async command-loop player.
#[derive(Debug)]
pub struct Player {
    config: PlayerConfig,
    state: PlaybackState,
    command_rx: mpsc::Receiver<PlaybackCommand>,
    event_tx: mpsc::Sender<PlayerEvent>,
}

/// Cloneable sender for [`PlaybackCommand`] values.
#[derive(Clone, Debug)]
pub struct PlayerHandle {
    command_tx: mpsc::Sender<PlaybackCommand>,
}

impl PlayerHandle {
    /// Sends a load command.
    pub async fn load(&self, source: MediaSource) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Load(source)).await
    }

    /// Sends a play command.
    pub async fn play(&self) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Play).await
    }

    /// Sends a pause command.
    pub async fn pause(&self) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Pause).await
    }

    /// Sends a stop command.
    pub async fn stop(&self) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Stop).await
    }

    /// Sends a seek command.
    pub async fn seek(&self, position: Duration) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Seek(position)).await
    }

    /// Sends a shutdown command.
    pub async fn shutdown(&self) -> Result<(), PlayerError> {
        self.send(PlaybackCommand::Shutdown).await
    }

    async fn send(&self, command: PlaybackCommand) -> Result<(), PlayerError> {
        self.command_tx
            .send(command)
            .await
            .map_err(|_| PlayerError::command_channel_closed())
    }
}

impl Player {
    /// Creates a player, its handle, and the event receiver.
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

    /// Returns the player configuration.
    pub fn config(&self) -> &PlayerConfig {
        &self.config
    }

    /// Runs the command loop until the command channel closes or shutdown is requested.
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
                    .map_err(|_| PlayerError::event_channel_closed())?;
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
                    .map_err(|_| PlayerError::event_channel_closed())?;
                Ok(true)
            }
            PlaybackCommand::Shutdown => {
                self.event_tx
                    .send(PlayerEvent::Shutdown)
                    .await
                    .map_err(|_| PlayerError::event_channel_closed())?;
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
            .map_err(|_| PlayerError::event_channel_closed())
    }
}
