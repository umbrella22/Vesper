use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("player command channel closed")]
    CommandChannelClosed,
    #[error("player event channel closed")]
    EventChannelClosed,
}
