//! config-digger core: everything that matters for safety lives here, behind
//! [`api`]. Front-ends parse input, render output and collect acknowledgements.

pub mod api;
pub mod definitions;
pub mod edit;
pub mod ports;

mod disable;
mod execute;
mod explain;
mod history;
mod locator;
mod mask;
mod paths;
mod plan;
mod resolve;
mod scan;
mod select;
mod skill;
