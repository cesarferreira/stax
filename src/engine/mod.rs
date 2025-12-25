pub mod metadata;
pub mod stack;

pub use metadata::BranchMetadata;
pub use stack::Stack;

// Re-export for tests
#[cfg(test)]
pub use stack::StackBranch;
