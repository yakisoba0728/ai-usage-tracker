//! Shared login plumbing used by both login flows: the cancel-flag lifecycle
//! (`cancel_token`) and the persist-and-emit completion step (`finish`).

pub mod cancel_token;
pub mod finish;
