mod common;
use anyhow::Result;
use axum::{Json, Router, extract::State, routing::post};
use joppa::{embeddings::Embedder, store};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

include!("search/part01.rs");
include!("search/part02.rs");
include!("search/part02b.rs");
