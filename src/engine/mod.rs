pub mod branch_detect;
pub mod metadata;
pub mod picker;
pub mod restack_preflight;
pub mod snapshot;
pub mod stack;

pub use metadata::{BranchMetadata, PrInfo};
pub use picker::build_parent_candidates;
pub use snapshot::StackSnapshot;
pub use stack::Stack;
