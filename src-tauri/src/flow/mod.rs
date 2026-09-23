pub mod cursor_source;
pub mod event;
pub mod focus_macos;
pub mod metrics;
pub mod pipeline;
pub mod privacy;
pub mod store;

pub use event::{FlowEvent, FlowEventType, FlowSource};
pub use pipeline::FlowPipeline;
pub use store::{FlowSettings, FlowStore};

use std::path::PathBuf;

pub fn default_flow_db_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("com.neilxu.agent-hud").join("flow.sqlite"))
}
