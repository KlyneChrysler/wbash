pub mod batch;
pub mod check;
pub mod fs;
pub mod git;
pub mod run;
pub mod search;
pub mod util;

use std::sync::Arc;

use wbash_core::registry::{Registry, Tool};

pub fn tools() -> Vec<Arc<dyn Tool>> {
    let mut out: Vec<Arc<dyn Tool>> = Vec::new();
    fs::register(&mut out);
    search::register(&mut out);
    run::register(&mut out);
    check::register(&mut out);
    git::register(&mut out);
    batch::register(&mut out);
    out
}

pub fn registry() -> Registry {
    Registry::new(tools())
}
