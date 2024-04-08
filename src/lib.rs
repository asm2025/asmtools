pub mod ai;
pub mod date;
pub mod error;
pub mod io;
pub mod logging;
#[cfg(feature = "mail")]
pub mod mail;
pub mod numeric;
#[cfg(feature = "python")]
pub mod python;
pub mod string;
pub mod threading;
pub mod web;

pub use self::app::*;

mod app;

pub(crate) trait ThreadSafe: Send + Sync {}
pub(crate) trait ThreadClonable: ThreadSafe + Clone {}
pub(crate) trait ThreadStatic: ThreadClonable + 'static {}
