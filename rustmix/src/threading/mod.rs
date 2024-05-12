mod cond;
pub use self::cond::*;
mod consumer;
pub use self::consumer::*;
mod injector_consumer;
pub use self::injector_consumer::*;
mod parallel_consumer;
pub use self::parallel_consumer::*;
mod producer_consumer;
pub use self::producer_consumer::*;
mod spinner;
pub use self::spinner::*;

use futures::Future;
use std::{fmt, pin::Pin, sync::Arc};
use tokio::{
    select,
    sync::Notify,
    time::{sleep, Duration},
};

use crate::{
    error::{CancelledError, TimedoutError},
    Result,
};

const CAPACITY_DEF: usize = 0;
const THREADS_DEF: usize = 1;
pub const THREADS_MIN: usize = 1;
pub const THREADS_MAX: usize = 255;
const QUEUE_BEHAVIOR_DEF: QueueBehavior = QueueBehavior::FIFO;
const THRESHOLD_DEF: Duration = Duration::ZERO;
const SLEEP_AFTER_SEND_DEF: Duration = Duration::ZERO;
const PEEK_TIMEOUT_DEF: Duration = Duration::from_millis(50);
const PEEK_TIMEOUT_MIN: Duration = Duration::from_millis(10);
const PEEK_TIMEOUT_MAX: Duration = Duration::from_secs(5);
const PAUSE_TIMEOUT_DEF: Duration = Duration::from_millis(50);
const PAUSE_TIMEOUT_MIN: Duration = Duration::from_millis(10);
const PAUSE_TIMEOUT_MAX: Duration = Duration::from_secs(5);
const SELECT_TIMEOUT: Duration = Duration::from_millis(10);
pub const INTERVAL: u64 = 100;

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskResult {
    #[default]
    None,
    Cancelled,
    TimedOut,
    Error(String),
    Success,
}

impl fmt::Display for TaskResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TaskResult::Cancelled => write!(f, "Cancelled"),
            TaskResult::TimedOut => write!(f, "Timedout"),
            TaskResult::Error(e) => write!(f, "Error: {}", e),
            TaskResult::Success => write!(f, "Success"),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QueueBehavior {
    #[default]
    FIFO,
    LIFO,
}

impl fmt::Display for QueueBehavior {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            QueueBehavior::FIFO => write!(f, "FIFO"),
            QueueBehavior::LIFO => write!(f, "LIFO"),
        }
    }
}

pub trait TaskDelegationBase<TD: Send + Sync + Clone + 'static, T: Send + Clone + 'static> {
    fn on_started(&self, td: &TD);
    fn on_completed(&self, td: &TD, item: &T, result: &TaskResult) -> bool;
    fn on_cancelled(&self, td: &TD);
    fn on_finished(&self, td: &TD);
}

pub trait TaskDelegation<TD: Send + Sync + Clone + 'static, T: Send + Clone + 'static>:
    TaskDelegationBase<TD, T>
{
    fn process(&self, td: &TD, item: &T) -> Result<TaskResult>;
}

pub trait AsyncTaskDelegation<TD: Send + Sync + Clone + 'static, T: Send + Clone + 'static>:
    TaskDelegationBase<TD, T>
{
    fn process(&self, td: &TD, item: &T) -> impl Future<Output = Result<TaskResult>> + Send;
}

pub trait AwaitableConsumer {
    fn is_cancelled(&self) -> bool;
    fn is_finished(&self) -> bool;
}

fn wait<T: AwaitableConsumer>(this: &T, finished: &Arc<Mutcond>) -> Result<()> {
    match finished.wait_while(|| !this.is_cancelled() && !this.is_finished()) {
        Ok(_) => {
            if this.is_cancelled() {
                Err(CancelledError.into())
            } else {
                Ok(())
            }
        }
        Err(_) => Err(CancelledError.into()),
    }
}

async fn wait_async<T: AwaitableConsumer>(this: &T, finished: &Arc<Notify>) -> Result<()> {
    while !this.is_finished() && !this.is_cancelled() {
        select! {
            _ = finished.notified() => {},
            else => {},
        }
    }

    if this.is_cancelled() {
        return Err(CancelledError.into());
    }

    Ok(())
}

fn wait_until<T: AwaitableConsumer>(
    this: &T,
    finished: &Arc<Mutcond>,
    cond: impl Fn(&T) -> bool,
) -> Result<()> {
    match finished.wait_while(|| !this.is_cancelled() && !this.is_finished() && !cond(this)) {
        Ok(_) => {
            if this.is_cancelled() {
                Err(CancelledError.into())
            } else {
                Ok(())
            }
        }
        Err(_) => Err(CancelledError.into()),
    }
}

async fn wait_until_async<
    T: AwaitableConsumer,
    F: Fn(&T) -> Pin<Box<dyn Future<Output = bool> + Send>>,
>(
    this: &T,
    finished: &Arc<Notify>,
    cond: F,
) -> Result<()> {
    while !this.is_cancelled() && !this.is_finished() {
        select! {
            _ = finished.notified() => {},
            _ = cond(this) => {},
            _ = sleep(SELECT_TIMEOUT) => {}
        }
    }

    if this.is_cancelled() {
        return Err(CancelledError.into());
    }

    Ok(())
}

fn wait_for<T: AwaitableConsumer>(
    this: &T,
    timeout: Duration,
    finished: &Arc<Mutcond>,
) -> Result<()> {
    if timeout.is_zero() {
        return Err(TimedoutError.into());
    }

    match finished.wait_timeout_while(|| !this.is_cancelled() && !this.is_finished(), timeout) {
        Ok(_) => {
            if this.is_cancelled() {
                Err(CancelledError.into())
            } else {
                Ok(())
            }
        }
        Err(_) => Err(TimedoutError.into()),
    }
}

async fn wait_for_async<T: AwaitableConsumer>(
    this: &T,
    timeout: Duration,
    finished: &Arc<Notify>,
) -> Result<()> {
    if timeout.is_zero() {
        return Err(TimedoutError.into());
    }

    select! {
        _ = finished.notified() => {
            if this.is_cancelled() {
                Err(CancelledError.into())
            } else {
            Ok(())
            }
        },
        _ = sleep(timeout) => Err(TimedoutError.into()),
    }
}

fn wait_for_until<T: AwaitableConsumer>(
    this: &T,
    timeout: Duration,
    finished: &Arc<Mutcond>,
    cond: impl Fn(&T) -> bool,
) -> Result<()> {
    if timeout.is_zero() {
        return Err(TimedoutError.into());
    }
    match finished.wait_timeout_while(
        || !this.is_cancelled() && !this.is_finished() && !cond(this),
        timeout,
    ) {
        Ok(_) => {
            if this.is_cancelled() {
                Err(CancelledError.into())
            } else {
                Ok(())
            }
        }
        Err(_) => Err(TimedoutError.into()),
    }
}

async fn wait_for_until_async<
    T: AwaitableConsumer,
    F: Fn(&T) -> Pin<Box<dyn Future<Output = bool> + Send>>,
>(
    this: &T,
    timeout: Duration,
    finished: &Arc<Notify>,
    cond: F,
) -> Result<()> {
    if timeout.is_zero() {
        return Err(TimedoutError.into());
    }

    select! {
        _ = finished.notified() => {
            if this.is_cancelled() {
                Err(CancelledError.into())
            } else {
                Ok(())
            }
        },
        _ = cond(this) => {
            if this.is_cancelled() {
                Err(CancelledError.into())
            } else {
                Ok(())
            }
        },
        _ = sleep(timeout) => Err(TimedoutError.into())
    }
}
