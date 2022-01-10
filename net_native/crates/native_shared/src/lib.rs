mod shared;

#[cfg(feature = "native")]
mod native;

#[cfg(feature = "native")]
mod logic;

#[cfg(feature = "native")]
pub use native::*;

pub use shared::*;