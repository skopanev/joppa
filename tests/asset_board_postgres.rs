//! An asset board field: the result names an object of the type asset, each criterion
//! lists the assets that apply to it or the explicit value all, and each asset gets its
//! own status. A new asset starts grey.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

include!("asset_board_postgres/part01.rs");
include!("asset_board_postgres/part02.rs");
