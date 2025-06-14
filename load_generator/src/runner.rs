use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Duration, interval, timeout};
use crate::client::{make_client, make_request};

// TODO: Should these be re-exported as inner types?
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use http_body_util::Full;
use hyper::body::Bytes;

const MAX_SYSTEM_QUEUE: usize = 100000;

struct ScheduledBatch {
    size: u64,
}

// TODO: This should probably be defined in result parser
struct UpstreamResult {
    response_code: u8,
    duration: u64,
}

// Rejig the load generator such that batches of requests are made every 100ms
// e.g. 1000 RPS --> 100 requests in 100ms
// We have to specialcase scenarios where <1 request is made every 100ms
const RESOLUTION_MILLIS: u64 = 100;

async fn setup_batches(tx: mpsc::Sender<ScheduledBatch>, requests_per_batch: u32, duration: u64) {
    let mut bucket_num = 1_u64;
    // Calculate all the number of batches we need upfront
    let total_batches = (duration * 1000) / RESOLUTION_MILLIS;
    println!("Total batches for duration: {}", total_batches);

    // 1000 Requests per 1000 ms means our batch size is <(RESOLUTION_MILLIS  * RPS ) / 1000 ms> --> 100 is our batch size
    let nominal_batch_size = (RESOLUTION_MILLIS as f32 * requests_per_batch as f32) / 1000_f32;
    let mut nth_bucket_for_small_rps: u64 = 0; // by default we'll assume this can be ignored and that for every bucket (100ms) we will have > 0 requests to make
    if nominal_batch_size < 1_f32 {
        // If the RPS is small enough - then only every Nth bucket will have a batch size of 1.
        // i.e. for 1RPS - with a resolution of 100ms; it means that 1 request is made every 10th bucket.
        // We need to know this value "10" by determining ceil(1 / nominal_batch_size).
        nth_bucket_for_small_rps = (1_f32 / nominal_batch_size) as u64;
    }

    // Create exactly the number of batches needed
    for _ in 0..total_batches {
        let this_batch: ScheduledBatch;
        // If we have >= 10 RPS - there's always something to do for our resolution of 100ms
        if nth_bucket_for_small_rps == 0 {
            this_batch = ScheduledBatch {
                size: nominal_batch_size as u64,
            };
        } else {
            // Otherwise - for low enough RPS; only every Nth bucket will make ONE request
            if bucket_num % nth_bucket_for_small_rps == 0 {
                this_batch = ScheduledBatch { size: 1 }
            } else {
                this_batch = ScheduledBatch { size: 0 }
            }
        }

        if let Err(e) = tx.send(this_batch).await {
            println!("Something went wrong trying to enqueue batch! {:#?}", e)
        }

        bucket_num += 1;
    }
}
pub async fn start_test(duration: u64, rps: u32) {
    // Channel is FIFO
    let (tx, rx) = mpsc::channel::<ScheduledBatch>(MAX_SYSTEM_QUEUE);

    let mut set = JoinSet::new();

    set.spawn(async move {
        let results = consume_batches(rx).await;
        for i in results {
            i.await.ok();
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
    let mut interval = tokio::time::interval(Duration::from_millis(RESOLUTION_MILLIS as u64));
    // We use Delay for missed intervals because its more important to preserve the RESOLUTION_MILLIS pace from wherever we last executed
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await; // starts at 0

    let mut batch_counter = 0;
    let mut batch_handles: Vec<JoinHandle<()>> = Vec::new();
    let mut last_time = Instant::now();

    // Building the client
    let client = make_client().await;

    loop {
        match rx.recv().await {
            Some(this_batch) => {
                batch_counter += 1;
                let current_time = Instant::now();
                let time_delta_millis = current_time.duration_since(last_time).as_millis();
                if time_delta_millis > RESOLUTION_MILLIS as u128 {
                    println!("Throwing away this delayed batch - late by: {}ms", time_delta_millis);
                    // TODO: Bump the number of dropped requests
                    continue
                }
                let batch_handle = tokio::spawn(make_upstream_requests(
                    this_batch.size,
                    client.clone(),
                    batch_counter,
                    last_time,
                    current_time,
                ));
                batch_handles.push(batch_handle);
                interval.tick().await;
                last_time = current_time;
            }
            None => break,
        }
    }
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
    for i in 0..batch_size {
        
        // simulate request
        //let random_duration = rand::rng().random_range(5..=80);
        //tokio::time::sleep(Duration::from_millis(random_duration)).await;

        make_request(String::from("localhost:8080"), client.clone()).await;
    }
}
