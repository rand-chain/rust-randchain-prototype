extern crate serialization as ser;
extern crate serialization_derive;
#[macro_use]
extern crate lazy_static;

mod config;
mod util;
mod vdf;

pub use crate::config::MODULUS;
pub use crate::vdf::Proof;
pub use crate::vdf::{eval, prove, verify};
