use std::{
    error::Error,
    sync::{atomic::AtomicUsize, Arc},
    thread,
};

use rustmix::threading::{consumer::*, injector_consumer::*, producer_consumer::*, *};

const THREADS: usize = 4;
const TEST_SIZE: usize = 10000;

#[derive(Debug, Clone)]
struct TaskHandler {
    pub task_count: Arc<AtomicUsize>,
    pub done_count: Arc<AtomicUsize>,
}

impl TaskHandler {
    pub fn new() -> Self {
        TaskHandler {
            task_count: Arc::new(AtomicUsize::new(0)),
            done_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ProducerConsumerDelegation<usize> for TaskHandler {
    fn process_task(
        &self,
        _pc: &ProducerConsumer<usize>,
        item: usize,
    ) -> Result<TaskResult, Box<dyn Error>> {
        let current_thread = thread::current();
        self.task_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        println!(
            "Item: {} in thread: {}",
            item,
            current_thread.name().unwrap()
        );

        if item % 5 == 0 {
            return Ok(TaskResult::Error(format!(
                "Item {}. Multiples of 5 are not allowed",
                item
            )));
        } else if item % 3 == 0 {
            return Ok(TaskResult::TimedOut);
        }

        Ok(TaskResult::Success)
    }

    fn on_task_completed(
        &self,
        _pc: &ProducerConsumer<usize>,
        item: usize,
        result: TaskResult,
    ) -> bool {
        let current_thread = thread::current();
        self.done_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        println!(
            "Result item: {}: {:?} in thread: {}",
            item,
            result,
            current_thread.name().unwrap()
        );
        true
    }

    fn on_finished(&self, _pc: &ProducerConsumer<usize>) {
        println!(
            "Got: {} tasks and finished {} tasks.",
            self.task_count.load(std::sync::atomic::Ordering::Relaxed),
            self.done_count.load(std::sync::atomic::Ordering::Relaxed)
        );
    }
}

impl ConsumerDelegation<usize> for TaskHandler {
    fn process_task(
        &self,
        _pc: &Consumer<usize>,
        item: usize,
    ) -> Result<TaskResult, Box<dyn Error>> {
        let current_thread = thread::current();
        self.task_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        println!(
            "Item: {} in thread: {}",
            item,
            current_thread.name().unwrap()
        );

        if item % 5 == 0 {
            return Ok(TaskResult::Error(format!(
                "Item {}. Multiples of 5 are not allowed",
                item
            )));
        } else if item % 3 == 0 {
            return Ok(TaskResult::TimedOut);
        }

        Ok(TaskResult::Success)
    }

    fn on_task_completed(&self, _pc: &Consumer<usize>, item: usize, result: TaskResult) -> bool {
        let current_thread = thread::current();
        self.done_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        println!(
            "Result item: {}: {:?} in thread: {}",
            item,
            result,
            current_thread.name().unwrap()
        );
        true
    }

    fn on_finished(&self, _pc: &Consumer<usize>) {
        println!(
            "Got: {} tasks and finished {} tasks.",
            self.task_count.load(std::sync::atomic::Ordering::Relaxed),
            self.done_count.load(std::sync::atomic::Ordering::Relaxed)
        );
    }
}

impl InjectorWorkerDelegation<usize> for TaskHandler {
    fn process_task(
        &self,
        _pc: &InjectorWorker<usize>,
        item: usize,
    ) -> Result<TaskResult, Box<dyn Error>> {
        let current_thread = thread::current();
        self.task_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        println!(
            "Item: {} in thread: {}",
            item,
            current_thread.name().unwrap()
        );

        if item % 5 == 0 {
            return Ok(TaskResult::Error(format!(
                "Item {}. Multiples of 5 are not allowed",
                item
            )));
        } else if item % 3 == 0 {
            return Ok(TaskResult::TimedOut);
        }

        Ok(TaskResult::Success)
    }

    fn on_task_completed(
        &self,
        _pc: &InjectorWorker<usize>,
        item: usize,
        result: TaskResult,
    ) -> bool {
        let current_thread = thread::current();
        self.done_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        println!(
            "Result item: {}: {:?} in thread: {}",
            item,
            result,
            current_thread.name().unwrap()
        );
        true
    }

    fn on_finished(&self, _pc: &InjectorWorker<usize>) {
        println!(
            "Got: {} tasks and finished {} tasks.",
            self.task_count.load(std::sync::atomic::Ordering::Relaxed),
            self.done_count.load(std::sync::atomic::Ordering::Relaxed)
        );
    }
}

pub async fn test_producer_consumer() -> Result<(), Box<dyn Error>> {
    println!("\nTesting Producer/Consumer with {} threads...", THREADS);

    let now = std::time::Instant::now();
    let handler = TaskHandler::new();
    let options = ProducerConsumerOptions::new();
    let prodcon = ProducerConsumer::<usize>::with_options(options);
    prodcon.start_producer(handler.clone());

    for _ in 0..THREADS {
        let con = handler.clone();
        prodcon.start_consumer(con);
    }

    for i in 1..=TEST_SIZE {
        prodcon.enqueue(i);
    }

    prodcon.complete();
    let _ = prodcon.wait_async().await;

    println!("Elapsed time: {:?}", now.elapsed());
    Ok(())
}

pub async fn test_consumer() -> Result<(), Box<dyn Error>> {
    println!("\nTesting Consumer with {} threads...", THREADS);

    let now = std::time::Instant::now();
    let handler = TaskHandler::new();
    let options = ConsumerOptions::new();
    let consumer = Consumer::<usize>::with_options(options);

    for _ in 0..THREADS {
        let con = handler.clone();
        consumer.start(con);
    }

    for i in 1..=TEST_SIZE {
        consumer.enqueue(i);
    }

    consumer.complete();
    let _ = consumer.wait_async().await;

    println!("Elapsed time: {:?}", now.elapsed());
    Ok(())
}

pub async fn test_injector_worker() -> Result<(), Box<dyn Error>> {
    println!("\nTesting Injector/Worker with {} threads...", THREADS);

    let now = std::time::Instant::now();
    let handler = TaskHandler::new();
    let options = InjectorWorkerOptions::new().with_threads(THREADS);
    let injwork = InjectorWorker::<usize>::with_options(options);
    injwork.start(handler);

    for i in 1..=TEST_SIZE {
        injwork.enqueue(i);
    }

    injwork.complete();
    let _ = injwork.wait_async().await;

    println!("Elapsed time: {:?}", now.elapsed());
    Ok(())
}
