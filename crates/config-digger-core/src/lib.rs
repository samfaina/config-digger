//! config-digger core: everything that matters for safety lives here, behind
//! [`api`]. Front-ends parse input, render output and collect acknowledgements.

pub mod api;
pub mod definitions;
pub mod edit;
pub mod ports;

mod disable;
mod execute;
mod explain;
mod folders;
mod history;
mod installations;
mod locator;
mod mask;
mod name_rule;
mod park;
mod paths;
mod plan;
mod resolve;
mod rules;
mod running;
mod scan;
mod select;
mod settings;
mod skill;
mod store;
mod updates;
mod user_list;
mod version;
mod walk;
