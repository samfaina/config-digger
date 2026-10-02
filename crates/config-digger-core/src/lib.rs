//! config-digger core: everything that matters for safety lives here, behind
//! [`api`]. Front-ends parse input, render output and collect acknowledgements.

pub mod api;
pub mod definitions;
pub mod edit;
pub mod ports;

mod explain;
mod locator;
mod paths;
mod resolve;
mod scan;
mod select;
