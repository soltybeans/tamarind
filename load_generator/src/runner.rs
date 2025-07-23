use crate::client::{make_client, make_request};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Duration, timeout};

// TODO: Should these be re-exported as inner types?
use http_body_util::Full;
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;

const MAX_SYSTEM_QUEUE: usize = 100000;

struct ScheduledBatch {
    size: u64,
    batch_number: u64,
}

//  We only consider request batches of 100millis i.e. <size>, or one of (0 or 1) in the case of [1,9] RPS -->
const RESOLUTION_MILLIS: u64 = 100;

async fn setup_batches(tx: mpsc::Sender<ScheduledBatch>, rps: u32, duration: u64) {
    let total_batches = (duration * 1000) / RESOLUTION_MILLIS;

    let nominal_batch_size = (RESOLUTION_MILLIS as f32 * rps as f32) / 1000_f32;
    let nth_bucket_for_small_rps = if nominal_batch_size < 1.0 {
        // If the RPS is small enough - then only every Nth bucket will have a batch size of 1.
        (1.0 / nominal_batch_size).ceil() as u64
    } else {
        0
    };

    // Create exactly the number of batches needed
    for i in 0..total_batches {
        let this_batch: ScheduledBatch;
        let bucket_num = i + 1;
        if nth_bucket_for_small_rps == 0 {
            this_batch = ScheduledBatch {
                size: nominal_batch_size as u64,
                batch_number: bucket_num,
            };
        } else {
            // Otherwise - for low enough RPS (e.g. 1...9 RPS); only every Nth bucket will make ONE request
            if bucket_num % nth_bucket_for_small_rps == 0 {
                this_batch = ScheduledBatch {
                    size: 1,
                    batch_number: bucket_num,
                }
            } else {
                this_batch = ScheduledBatch {
                    size: 0,
                    batch_number: bucket_num,
                }
            }
        }

        if let Err(e) = tx.send(this_batch).await {
            println!("Something went wrong trying to enqueue batch! {:#?}", e)
        }
    }
}
pub async fn start_test(duration: u64, rps: u32) {
    // Channel is FIFO
    let (tx, rx) = mpsc::channel::<ScheduledBatch>(MAX_SYSTEM_QUEUE);

    let mut set = JoinSet::new();

    set.spawn(async move {
        let results = consume_batches(rx).await;
        for i in results {
            i.await.ok(); // waiting for the batches of requests to be done
        }
    });
    set.spawn(async move {
        let _ = timeout(
            Duration::from_secs(duration),
            setup_batches(tx, rps, duration),
        )
        .await;
    });

    set.join_all().await;
}

async fn consume_batches(mut rx: mpsc::Receiver<ScheduledBatch>) -> Vec<JoinHandle<()>> {
    let mut interval = tokio::time::interval(Duration::from_millis(RESOLUTION_MILLIS));
    // We use Delay for missed intervals because its more important to preserve the RESOLUTION_MILLIS pace from wherever we last executed
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await; // starts at 0

    let mut batch_counter = 0;
    let mut batch_handles: Vec<JoinHandle<()>> = Vec::new();
    let mut last_time = Instant::now();

    let mut dropped_requests = 0;
    // Building the client
    let client = make_client().await;

    loop {
        interval.tick().await;
        match rx.recv().await {
            Some(this_batch) => {
                batch_counter += 1;
                let current_time = Instant::now();
                let time_delta_millis = current_time.duration_since(last_time).as_millis();
                if time_delta_millis > RESOLUTION_MILLIS as u128 {
                    println!(
                        "Throwing away this delayed batch - late by: {}ms",
                        time_delta_millis
                    );
                    dropped_requests += this_batch.size;
                    last_time = current_time; // even if we drop requests, we last "saw" a batch now
                    continue;
                }
                let batch_handle = tokio::spawn(make_upstream_requests(
                    this_batch.size,
                    client.clone(),
                    batch_counter,
                    last_time,
                    current_time,
                ));
                batch_handles.push(batch_handle);
                last_time = current_time;
            }
            None => break, // channel is closed
        }
    }
    println!("Total dropped requests: {}", dropped_requests);
    batch_handles
}

async fn make_upstream_requests(
    batch_size: u64,
    client: Client<HttpConnector, Full<Bytes>>,
    batch_counter: i32,
    last_time: Instant,
    current_time: Instant,
) {
    println!("--------------------------------------------");
    println!(
        "[Batch {:#?}] -  Time  now: {:#?} - Batch size: {:#?} - Time delta [ms]: {:#?}",
        batch_counter,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        batch_size,
        current_time.duration_since(last_time).as_millis()
    );

    for _ in 0..batch_size {
        make_request(String::from("localhost:8080"), client.clone()).await;
    }
}
