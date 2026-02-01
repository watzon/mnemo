//! Mnemo - Semantic memory layer for LLM applications
//!
//! This crate provides a daemon that manages hierarchical memory storage
//! with semantic search capabilities using vector embeddings.

pub mod cli;
pub mod config;
pub mod embedding;
pub mod error;
pub mod memory;
pub mod proxy;
pub mod router;
pub mod storage;
pub mod testing;

pub use error::MnemoError;
