use std::{
    error::Error,
    sync::{atomic::AtomicUsize, Arc},
    thread,
    time::Duration,
};

use rustmix::threading::{consumer::*, producer_consumer::*, *};

#[derive(Debug, Clone)]
struct ProCon {
    pub task_count: Arc<AtomicUsize>,
    pub done_count: Arc<AtomicUsize>,
}

impl ProCon {
    pub fn new() -> Self {
        ProCon {
            task_count: Arc::new(AtomicUsize::new(0)),
            done_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl ProducerConsumerDelegation<usize> for ProCon {
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

impl ConsumerDelegation<usize> for ProCon {
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

pub async fn test_producer_consumer(threads: usize) -> Result<(), Box<dyn Error>> {
    let th = if threads > 0 { threads } else { 1 };
    println!("\nTesting Producer/Consumer with {} threads...", th);

    let now = std::time::Instant::now();
    let prodcon = ProCon::new();
    let options = ProducerConsumerOptions::new();
    let pc = ProducerConsumer::<usize>::with_options(options);
    pc.start_producer(prodcon.clone());

    for _ in 0..th {
        let con = prodcon.clone();
        pc.start_consumer(con);
    }

    for i in 1..=10000 {
        pc.enqueue(i);
    }

    pc.complete();
    let _ = pc.wait_async().await;

    println!("Elapsed time: {:?}", now.elapsed());
    Ok(())
}

pub async fn test_consumer(threads: usize) -> Result<(), Box<dyn Error>> {
    let th = if threads > 0 { threads } else { 1 };
    println!("\nTesting Consumer with {} threads...", th);

    let now = std::time::Instant::now();
    let consumer = ProCon::new();
    let options = ConsumerOptions::new();
    let c = Consumer::<usize>::with_options(options);

    for _ in 0..th {
        let con = consumer.clone();
        c.start(con);
    }

    for i in 1..=10000 {
        c.enqueue(i);
    }

    c.complete();
    let _ = c.wait_async().await;

    println!("Elapsed time: {:?}", now.elapsed());
    Ok(())
}
