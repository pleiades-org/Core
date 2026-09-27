mod execute_search;
mod normalize;
mod parse_query;
mod power;
mod recent_applications;
mod run_target;
mod taskbar;
mod terminal;
pub use power::PowerAction;
pub use terminal::{RunMode, ShellKind};

pub use execute_search::{Action, ResultKind, SearchBatch, SearchEngine, SearchResult};
pub use normalize::normalize;
pub use parse_query::{parse_query, CommandKind, ParsedQuery, QueryError, MAX_QUERY_BYTES};
