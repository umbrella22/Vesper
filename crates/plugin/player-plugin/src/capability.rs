use serde::{Deserialize, Serialize};

use crate::{ContentFormatKind, OutputFormat};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProcessorCapabilities {
    pub supported_input_formats: Vec<ContentFormatKind>,
    pub output_formats: Vec<OutputFormat>,
    pub supports_cancellation: bool,
}
