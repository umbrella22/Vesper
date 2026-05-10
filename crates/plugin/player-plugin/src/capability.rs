use serde::{Deserialize, Serialize};

use crate::{AssemblyMode, ContentFormatKind, OutputFormat};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProcessorCapabilities {
    pub supported_input_formats: Vec<ContentFormatKind>,
    pub output_formats: Vec<OutputFormat>,
    pub supports_cancellation: bool,
    #[serde(default)]
    pub supports_assembly: bool,
    #[serde(default)]
    pub supported_assembly_modes: Vec<AssemblyMode>,
}
