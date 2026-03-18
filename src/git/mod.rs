pub mod refs;
pub mod repo;

pub use repo::{checkout_branch_in, local_branch_exists_in, GitRepo, RebaseResult};
