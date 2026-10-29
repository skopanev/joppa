mod common;
use common::*;
use deadpool_postgres::Pool;
use joppa::store::{self, ObjectScope};
use serde_json::{Value, json};

include!("unstarted_work/part01.rs");
include!("unstarted_work/part02.rs");
include!("unstarted_work/part03.rs");
