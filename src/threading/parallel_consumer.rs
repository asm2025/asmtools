// use std::{
//     error::Error,
//     future::Future,
//     sync::{
//         atomic::{AtomicBool, AtomicUsize},
//         Arc, Condvar, Mutex,
//     },
//     thread,
//     time::{Duration, Instant},
// };
// use tokio::sync::Notify;

// use super::*;

// pub trait ParallelDelegation<T: Send + Clone + 'static> {
//     fn process(&self, pc: &Parallel<T>, item: &T) -> Result<TaskResult, Box<dyn Error>>;
//     fn on_completed(&self, pc: &Parallel<T>, item: &T, result: TaskResult) -> bool;
//     fn on_finished(&self, pc: &Parallel<T>);
// }

// #[derive(Debug, Clone, PartialEq, Eq)]
// pub struct ParallelOptions {
//     pub behavior: QueueBehavior,
//     pub threads: usize,
//     pub threshold: Duration,
//     pub sleep_after_send: Duration,
//     pub peek_timeout: Duration,
//     pub pause_timeout: Duration,
// }

// impl Default for ParallelOptions {
//     fn default() -> Self {
//         ParallelOptions {
//             behavior: QUEUE_BEHAVIOR_DEF,
//             threads: THREADS_DEF,
//             threshold: THRESHOLD_DEF,
//             sleep_after_send: SLEEP_AFTER_SEND_DEF,
//         }
//     }
// }

// impl ParallelOptions {
//     pub fn new() -> Self {
//         Default::default()
//     }

//     pub fn with_behavior(&self, behavior: QueueBehavior) -> Self {
//         ParallelOptions {
//             behavior,
//             ..self.clone()
//         }
//     }

//     pub fn with_threads(&self, threads: usize) -> Self {
//         ParallelOptions {
//             threads: if threads > 0 { threads } else { 1 },
//             ..self.clone()
//         }
//     }

//     pub fn with_threshold(&self, threshold: Duration) -> Self {
//         ParallelOptions {
//             threshold,
//             ..self.clone()
//         }
//     }

//     pub fn with_sleep_after_send(&self, sleep_after_send: Duration) -> Self {
//         ParallelOptions {
//             sleep_after_send,
//             ..self.clone()
//         }
//     }
// }

// #[derive(Clone, Debug)]
// pub struct Parallel<T: Send + Clone + 'static> {
//     options: ParallelOptions,
//     items: Arc<Mutex<LinkedList<T>>>,
//     items_cond: Arc<Condvar>,
//     finished: Arc<Mutex<bool>>,
//     finished_cond: Arc<Condvar>,
//     finished_notify: Arc<Notify>,
//     completed: Arc<AtomicBool>,
//     paused: Arc<AtomicBool>,
//     cancelled: Arc<AtomicBool>,
//     consumers_count: Arc<AtomicUsize>,
//     running_count: Arc<AtomicUsize>,
// }

// impl<T: Send + Clone> Consumer<T> {
//     pub fn new() -> Self {
//         let options: ConsumerOptions = Default::default();
//         Consumer {
//             options: options,
//             items: Arc::new(Mutex::new(LinkedList::new())),
//             items_cond: Arc::new(Condvar::new()),
//             finished: Arc::new(Mutex::new(false)),
//             finished_cond: Arc::new(Condvar::new()),
//             finished_notify: Arc::new(Notify::new()),
//             completed: Arc::new(AtomicBool::new(false)),
//             paused: Arc::new(AtomicBool::new(false)),
//             cancelled: Arc::new(AtomicBool::new(false)),
//             consumers_count: Arc::new(AtomicUsize::new(0)),
//             running_count: Arc::new(AtomicUsize::new(0)),
//         }
//     }

//     pub fn with_options(options: ConsumerOptions) -> Self {
//         Consumer {
//             options: options,
//             items: Arc::new(Mutex::new(LinkedList::new())),
//             items_cond: Arc::new(Condvar::new()),
//             finished: Arc::new(Mutex::new(false)),
//             finished_cond: Arc::new(Condvar::new()),
//             finished_notify: Arc::new(Notify::new()),
//             completed: Arc::new(AtomicBool::new(false)),
//             paused: Arc::new(AtomicBool::new(false)),
//             cancelled: Arc::new(AtomicBool::new(false)),
//             consumers_count: Arc::new(AtomicUsize::new(0)),
//             running_count: Arc::new(AtomicUsize::new(0)),
//         }
//     }

//     pub fn is_empty(&self) -> bool {
//         self.items.lock().unwrap().is_empty()
//     }

//     pub fn is_completed(&self) -> bool {
//         self.completed.load(std::sync::atomic::Ordering::Relaxed)
//     }

//     pub fn is_paused(&self) -> bool {
//         self.paused.load(std::sync::atomic::Ordering::Relaxed)
//     }

//     pub fn is_cancelled(&self) -> bool {
//         self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
//     }

//     pub fn is_busy(&self) -> bool {
//         (self
//             .consumers_count
//             .load(std::sync::atomic::Ordering::Relaxed)
//             > 0)
//             || (self
//                 .running_count
//                 .load(std::sync::atomic::Ordering::Relaxed)
//                 > 0)
//             || (self.items.lock().unwrap().len() > 0)
//     }

//     pub fn count(&self) -> usize {
//         self.items.lock().unwrap().len()
//     }

//     pub fn consumers(&self) -> usize {
//         self.consumers_count
//             .load(std::sync::atomic::Ordering::Relaxed)
//     }

//     fn inc_consumers(&self) {
//         self.consumers_count
//             .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
//     }

//     fn dec_consumers(&self, td: &dyn ConsumerDelegation<T>) {
//         self.consumers_count
//             .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
//         self.check_finished(td);
//     }

//     fn check_finished(&self, td: &dyn ConsumerDelegation<T>) {
//         if self.is_completed() && self.consumers() == 0 {
//             let mut finished = self.finished.lock().unwrap();
//             *finished = true;
//             td.on_finished(self);
//             self.finished_cond.notify_all();
//             self.finished_notify.notify_waiters();
//         }
//     }

//     pub fn running(&self) -> usize {
//         self.running_count
//             .load(std::sync::atomic::Ordering::Relaxed)
//     }

//     fn inc_running(&self) {
//         self.running_count
//             .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
//     }

//     fn dec_running(&self) {
//         self.running_count
//             .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
//     }

//     pub fn start<S: ConsumerDelegation<T> + Send + Clone + 'static>(&self, delegate: S) {
//         if self.is_cancelled() {
//             panic!("Queue is already cancelled.")
//         }

//         if self.is_completed() && self.is_empty() {
//             panic!("Queue is already completed.")
//         }

//         self.inc_consumers();
//         let this = Arc::new(self.clone());
//         let delegate = delegate.clone();
//         let builder = thread::Builder::new().name(format!("Consumer {}", self.consumers()));
//         builder
//             .spawn(move || {
//                 loop {
//                     if this.is_cancelled()
//                         || (this.is_empty() && this.is_completed() && this.running() == 0)
//                     {
//                         break;
//                     }

//                     if this.is_paused() {
//                         thread::sleep(this.options.pause_timeout);
//                         continue;
//                     }

//                     if let Some(item) = match this.options.behavior {
//                         QueueBehavior::FIFO => this.dequeue(),
//                         QueueBehavior::LIFO => this.pop(),
//                     } {
//                         this.inc_running();

//                         if let Ok(result) = delegate.process_task(&this, item.clone()) {
//                             if !delegate.on_task_completed(&this, item, result) {
//                                 this.dec_running();
//                                 break;
//                             }
//                         }

//                         this.dec_running();
//                     }
//                 }

//                 this.dec_consumers(&delegate);
//             })
//             .unwrap();
//     }

//     pub fn stop(&self, enforce: bool) {
//         if enforce {
//             self.cancel();
//         } else {
//             self.complete();
//         }
//     }

//     pub fn enqueue(&self, item: T) {
//         if self.is_cancelled() {
//             panic!("Queue is already cancelled.")
//         }

//         if self.is_completed() {
//             panic!("Queue is already completed.")
//         }

//         let mut items = self.items.lock().unwrap();
//         items.push_back(item);

//         if self.options.sleep_after_send > Duration::ZERO {
//             thread::sleep(self.options.sleep_after_send);
//         }

//         self.items_cond.notify_one();
//     }

//     fn dequeue(&self) -> Option<T> {
//         let mut items = self.items.lock().unwrap();

//         while items.is_empty() && !self.is_cancelled() && !self.is_completed() {
//             let result = self
//                 .items_cond
//                 .wait_timeout(items, self.options.peek_timeout)
//                 .unwrap();
//             items = result.0;

//             if result.1.timed_out() {
//                 continue;
//             }

//             if self.is_cancelled() || self.is_completed() {
//                 return None;
//             }

//             return items.pop_front();
//         }

//         if items.is_empty() || self.is_cancelled() {
//             return None;
//         }

//         items.pop_front()
//     }

//     fn pop(&self) -> Option<T> {
//         let mut items = self.items.lock().unwrap();

//         while items.is_empty() && !self.is_cancelled() && !self.is_completed() {
//             let result = self
//                 .items_cond
//                 .wait_timeout(items, self.options.peek_timeout)
//                 .unwrap();
//             items = result.0;

//             if result.1.timed_out() {
//                 continue;
//             }

//             if self.is_cancelled() || self.is_completed() {
//                 return None;
//             }

//             return items.pop_back();
//         }

//         if items.is_empty() || self.is_cancelled() {
//             return None;
//         }

//         items.pop_back()
//     }

//     pub fn peek(&self) -> Option<T> {
//         let items = self.items.lock().unwrap();

//         if items.is_empty() {
//             return None;
//         }

//         if let Some(item) = match self.options.behavior {
//             QueueBehavior::FIFO => items.front(),
//             QueueBehavior::LIFO => items.back(),
//         } {
//             Some(item.clone())
//         } else {
//             None
//         }
//     }

//     pub fn clear(&self) {
//         let mut items = self.items.lock().unwrap();
//         items.clear();
//     }

//     pub fn complete(&self) {
//         self.completed
//             .store(true, std::sync::atomic::Ordering::Relaxed);
//         self.items_cond.notify_all();
//     }

//     pub fn cancel(&self) {
//         self.cancelled
//             .store(true, std::sync::atomic::Ordering::Relaxed);
//         self.items_cond.notify_all();
//     }

//     pub fn pause(&self) {
//         self.paused
//             .store(true, std::sync::atomic::Ordering::Relaxed);
//     }

//     pub fn resume(&self) {
//         self.paused
//             .store(false, std::sync::atomic::Ordering::Relaxed);
//         self.items_cond.notify_all();
//     }

//     pub fn wait(&self) {
//         let finished = self.finished.lock().unwrap();

//         if !*finished {
//             let _ignored = self.finished_cond.wait(finished).unwrap();
//         }
//     }

//     pub async fn wait_async(&self) {
//         while !*self.finished.lock().unwrap() {
//             self.finished_notify.notified().await;
//             thread::sleep(self.options.pause_timeout);
//         }
//     }

//     pub fn wait_for(&self, timeout: Duration) -> bool {
//         if timeout == Duration::ZERO {
//             self.wait();
//             return true;
//         }

//         let start = Instant::now();
//         let mut finished = self.finished.lock().unwrap();

//         while !*finished && start.elapsed() < timeout {
//             let result = self
//                 .finished_cond
//                 .wait_timeout(finished, self.options.pause_timeout)
//                 .unwrap();
//             finished = result.0;
//             thread::sleep(self.options.pause_timeout);

//             if result.1.timed_out() || start.elapsed() >= timeout {
//                 return false;
//             }
//         }

//         start.elapsed() < timeout
//     }

//     pub async fn wait_for_async(&self, timeout: Duration) -> Box<dyn Future<Output = bool>> {
//         if timeout == Duration::ZERO {
//             self.wait_async().await;
//             return Box::new(async { true });
//         }

//         let start = Instant::now();
//         let mut finished = self.finished.lock().unwrap();

//         while !*finished && start.elapsed() < timeout {
//             let result = self
//                 .finished_cond
//                 .wait_timeout(finished, self.options.pause_timeout)
//                 .unwrap();
//             finished = result.0;
//             thread::sleep(self.options.pause_timeout);

//             if result.1.timed_out() || start.elapsed() >= timeout {
//                 return Box::new(async move { false });
//             }
//         }

//         Box::new(async move { start.elapsed() < timeout })
//     }
// }