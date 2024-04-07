use anyhow::Result;
use std::{
    collections::LinkedList,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
};
use tokio::{
    sync::Notify,
    time::{Duration, Instant},
};

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerOptions {
    pub behavior: QueueBehavior,
    pub threads: usize,
    pub threshold: Duration,
    pub sleep_after_send: Duration,
    pub peek_timeout: Duration,
    pub pause_timeout: Duration,
}

impl Default for ConsumerOptions {
    fn default() -> Self {
        ConsumerOptions {
            behavior: QUEUE_BEHAVIOR_DEF,
            threads: THREADS_DEF.clamp(THREADS_MIN, THREADS_MAX),
            threshold: THRESHOLD_DEF,
            sleep_after_send: SLEEP_AFTER_SEND_DEF,
            peek_timeout: PEEK_TIMEOUT_DEF.clamp(PEEK_TIMEOUT_MIN, PEEK_TIMEOUT_MAX),
            pause_timeout: PAUSE_TIMEOUT_DEF.clamp(PAUSE_TIMEOUT_MIN, PAUSE_TIMEOUT_MAX),
        }
    }
}

impl ConsumerOptions {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_threads(&self, threads: usize) -> Self {
        ConsumerOptions {
            threads: threads.clamp(THREADS_MIN, THREADS_MAX),
            ..self.clone()
        }
    }

    pub fn with_behavior(&self, behavior: QueueBehavior) -> Self {
        ConsumerOptions {
            behavior,
            ..self.clone()
        }
    }

    pub fn with_threshold(&self, threshold: Duration) -> Self {
        ConsumerOptions {
            threshold,
            ..self.clone()
        }
    }

    pub fn with_sleep_after_send(&self, sleep_after_send: Duration) -> Self {
        ConsumerOptions {
            sleep_after_send,
            ..self.clone()
        }
    }
}

#[derive(Clone, Debug)]
pub struct Consumer<T: Send + Sync + Clone + 'static> {
    options: ConsumerOptions,
    items: Arc<Mutex<LinkedList<T>>>,
    items_cond: Arc<Condvar>,
    started: Arc<Mutex<bool>>,
    finished: Arc<AtomicBool>,
    finished_noti: Arc<Notify>,
    completed: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    consumers: Arc<AtomicUsize>,
    running: Arc<AtomicUsize>,
}

impl<T: Send + Sync + Clone> Consumer<T> {
    pub fn new() -> Self {
        Consumer {
            options: Default::default(),
            items: Arc::new(Mutex::new(LinkedList::new())),
            items_cond: Arc::new(Condvar::new()),
            started: Arc::new(Mutex::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            finished_noti: Arc::new(Notify::new()),
            completed: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            consumers: Arc::new(AtomicUsize::new(0)),
            running: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn with_options(options: ConsumerOptions) -> Self {
        Consumer {
            options,
            items: Arc::new(Mutex::new(LinkedList::new())),
            items_cond: Arc::new(Condvar::new()),
            started: Arc::new(Mutex::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            finished_noti: Arc::new(Notify::new()),
            completed: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            consumers: Arc::new(AtomicUsize::new(0)),
            running: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    pub fn is_started(&self) -> bool {
        *self.started.lock().unwrap()
    }

    fn set_started(&self, value: bool) -> bool {
        let mut started = self.started.lock().unwrap();

        if *started && value {
            return false;
        }

        *started = true;
        true
    }

    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    pub fn is_busy(&self) -> bool {
        self.count() > 0
    }

    pub fn count(&self) -> usize {
        self.items.lock().unwrap().len() + self.running.load(Ordering::SeqCst)
    }

    pub fn consumers(&self) -> usize {
        self.consumers.load(Ordering::SeqCst)
    }

    fn set_consumers(&self, value: usize) {
        self.consumers.store(value, Ordering::SeqCst);
    }

    fn dec_consumers(&self, td: &impl TaskDelegationBase<Consumer<T>, T>) {
        self.consumers.fetch_sub(1, Ordering::SeqCst);
        self.check_finished(td);
    }

    fn check_finished(&self, td: &impl TaskDelegationBase<Consumer<T>, T>) {
        if self.consumers() == 0 && (self.is_completed() || self.is_cancelled()) {
            self.completed.store(true, Ordering::SeqCst);
            self.finished.store(true, Ordering::SeqCst);

            if self.is_cancelled() {
                td.on_cancelled(self);
            } else {
                td.on_finished(self);
            }

            self.set_started(false);
            self.finished_noti.notify_one();
        }
    }

    pub fn running(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }

    fn inc_running(&self) {
        self.running.fetch_add(1, Ordering::SeqCst);
    }

    fn dec_running(&self) {
        self.running.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn start<TD: TaskDelegation<Consumer<T>, T> + Send + Sync + Clone + 'static>(
        &self,
        delegate: &TD,
    ) {
        if self.is_cancelled() {
            panic!("Queue is already cancelled.")
        }

        if self.is_completed() && self.is_empty() {
            panic!("Queue is already completed.")
        }

        if !self.set_started(true) {
            return;
        }

        self.set_consumers(self.options.threads);
        delegate.on_started(self);

        for _ in 0..self.options.threads {
            let this = Arc::new(self.clone());
            let delegate = delegate.clone();
            thread::spawn(move || {
                if this.options.threshold.is_zero() {
                    loop {
                        if this.is_cancelled() || (this.is_empty() && this.is_completed()) {
                            break;
                        }

                        if this.is_paused() {
                            thread::sleep(this.options.pause_timeout);
                            continue;
                        }

                        if let Some(item) = match this.options.behavior {
                            QueueBehavior::FIFO => this.dequeue(),
                            QueueBehavior::LIFO => this.pop(),
                        } {
                            this.inc_running();

                            match delegate.process(&this, &item) {
                                Ok(result) => {
                                    if !delegate.on_completed(&this, &item, &result) {
                                        this.dec_running();
                                        break;
                                    }
                                }
                                Err(e) => {
                                    if !delegate.on_completed(
                                        &this,
                                        &item,
                                        &TaskResult::Error(e.to_string()),
                                    ) {
                                        this.dec_running();
                                        break;
                                    }
                                }
                            }

                            this.dec_running();
                        }
                    }

                    this.dec_consumers(&delegate);
                    drop(delegate);
                    drop(this);
                    return;
                }

                loop {
                    if this.is_cancelled() || (this.is_empty() && this.is_completed()) {
                        break;
                    }

                    if this.is_paused() {
                        thread::sleep(this.options.pause_timeout);
                        continue;
                    }

                    if let Some(item) = match this.options.behavior {
                        QueueBehavior::FIFO => this.dequeue(),
                        QueueBehavior::LIFO => this.pop(),
                    } {
                        this.inc_running();

                        match delegate.process(&this, &item) {
                            Ok(result) => {
                                let time = Instant::now();

                                if !delegate.on_completed(&this, &item, &result) {
                                    this.dec_running();
                                    break;
                                }

                                if !this.options.threshold.is_zero()
                                    && time.elapsed() < this.options.threshold
                                {
                                    let remaining = this.options.threshold - time.elapsed();
                                    thread::sleep(remaining);
                                }
                            }
                            Err(e) => {
                                if !delegate.on_completed(
                                    &this,
                                    &item,
                                    &TaskResult::Error(e.to_string()),
                                ) {
                                    this.dec_running();
                                    break;
                                }
                            }
                        }

                        this.dec_running();
                    }
                }

                this.dec_consumers(&delegate);
                drop(delegate);
                drop(this);
            });
        }
    }

    pub async fn start_async<
        TD: AsyncTaskDelegation<Consumer<T>, T> + Send + Sync + Clone + 'static,
    >(
        &self,
        delegate: &TD,
    ) {
        if self.is_cancelled() {
            panic!("Queue is already cancelled.")
        }

        if self.is_completed() && self.is_empty() {
            panic!("Queue is already completed.")
        }

        if !self.set_started(true) {
            return;
        }

        self.set_consumers(self.options.threads);
        delegate.on_started(self);

        for _ in 0..self.options.threads {
            let this = Arc::new(self.clone());
            let delegate = delegate.clone();
            tokio::spawn(async move {
                if this.options.threshold.is_zero() {
                    loop {
                        if this.is_cancelled() || (this.is_empty() && this.is_completed()) {
                            break;
                        }

                        if this.is_paused() {
                            thread::sleep(this.options.pause_timeout);
                            continue;
                        }

                        if let Some(item) = match this.options.behavior {
                            QueueBehavior::FIFO => this.dequeue(),
                            QueueBehavior::LIFO => this.pop(),
                        } {
                            this.inc_running();

                            if let Ok(result) = delegate.process(&this, &item).await {
                                if !delegate.on_completed(&this, &item, &result) {
                                    this.dec_running();
                                    break;
                                }
                            }

                            this.dec_running();
                        }
                    }

                    this.dec_consumers(&delegate);
                    drop(delegate);
                    drop(this);
                    return;
                }

                loop {
                    if this.is_cancelled() || (this.is_empty() && this.is_completed()) {
                        break;
                    }

                    if this.is_paused() {
                        thread::sleep(this.options.pause_timeout);
                        continue;
                    }

                    if let Some(item) = match this.options.behavior {
                        QueueBehavior::FIFO => this.dequeue(),
                        QueueBehavior::LIFO => this.pop(),
                    } {
                        this.inc_running();

                        if let Ok(result) = delegate.process(&this, &item).await {
                            let time = Instant::now();

                            if !delegate.on_completed(&this, &item, &result) {
                                this.dec_running();
                                break;
                            }

                            if !this.options.threshold.is_zero()
                                && time.elapsed() < this.options.threshold
                            {
                                let remaining = this.options.threshold - time.elapsed();
                                thread::sleep(remaining);
                            }
                        }

                        this.dec_running();
                    }
                }

                this.dec_consumers(&delegate);
                drop(delegate);
                drop(this);
            });
        }
    }

    pub fn stop(&self, enforce: bool) {
        if enforce {
            self.cancel();
        } else {
            self.complete();
        }
    }

    pub fn enqueue(&self, item: T) {
        if self.is_cancelled() {
            return;
        }

        if self.is_completed() {
            panic!("Queue is already completed.")
        }

        let mut items = self.items.lock().unwrap();
        items.push_back(item);

        if !self.options.sleep_after_send.is_zero() {
            thread::sleep(self.options.sleep_after_send);
        }

        self.items_cond.notify_one();
    }

    fn dequeue(&self) -> Option<T> {
        let mut items = self.items.lock().unwrap();

        while items.is_empty() && !self.is_cancelled() && !self.is_completed() {
            let result = self
                .items_cond
                .wait_timeout(items, self.options.peek_timeout)
                .unwrap();
            items = result.0;

            if result.1.timed_out() {
                continue;
            }

            if self.is_cancelled() || self.is_completed() {
                return None;
            }

            return items.pop_front();
        }

        if items.is_empty() || self.is_cancelled() {
            return None;
        }

        items.pop_front()
    }

    fn pop(&self) -> Option<T> {
        let mut items = self.items.lock().unwrap();

        while items.is_empty() && !self.is_cancelled() && !self.is_completed() {
            let result = self
                .items_cond
                .wait_timeout(items, self.options.peek_timeout)
                .unwrap();
            items = result.0;

            if result.1.timed_out() {
                continue;
            }

            if self.is_cancelled() || self.is_completed() {
                return None;
            }

            return items.pop_back();
        }

        if items.is_empty() || self.is_cancelled() {
            return None;
        }

        items.pop_back()
    }

    pub fn peek(&self) -> Option<T> {
        let items = self.items.lock().unwrap();

        if items.is_empty() {
            return None;
        }

        if let Some(item) = match self.options.behavior {
            QueueBehavior::FIFO => items.front(),
            QueueBehavior::LIFO => items.back(),
        } {
            Some(item.clone())
        } else {
            None
        }
    }

    pub fn clear(&self) {
        let mut items = self.items.lock().unwrap();
        items.clear();
    }

    pub fn complete(&self) {
        self.completed.store(true, Ordering::SeqCst);
        self.items_cond.notify_all();
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.items_cond.notify_all();
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.items_cond.notify_all();
    }

    pub fn wait(&self) -> Result<()> {
        wait(self, self.options.pause_timeout, &self.finished_noti)
    }

    pub async fn wait_async(&self) -> Result<()> {
        wait_async(self, self.options.pause_timeout, &self.finished_noti).await
    }

    pub fn wait_for(&self, timeout: Duration) -> Result<bool> {
        wait_for(
            self,
            timeout,
            self.options.pause_timeout,
            &self.finished_noti,
        )
    }

    pub async fn wait_for_async(&self, timeout: Duration) -> Result<bool> {
        wait_for_async(
            self,
            timeout,
            self.options.pause_timeout,
            &self.finished_noti,
        )
        .await
    }
}

impl<T: Send + Sync + Clone> AwaitableConsumer for Consumer<T> {
    fn is_cancelled(&self) -> bool {
        Consumer::is_cancelled(self)
    }

    fn is_finished(&self) -> bool {
        Consumer::is_finished(self)
    }
}
