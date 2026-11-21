#![allow(non_snake_case, clippy::missing_safety_doc)]

pub mod ctx;
pub mod paths;
pub mod targets;
pub mod manager;
pub mod errsys;

#[cfg(test)]
mod alias_tests;
#[cfg(test)]
mod tests;
