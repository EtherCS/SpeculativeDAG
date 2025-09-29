// // Copyright (c) Mysten Labs, Inc.
// // SPDX-License-Identifier: Apache-2.0

// // ScheduleFetcher is responsible for fetching scheduled tasks from the PevmAPI.
// // The fetched tasks can then be proposed as a Mysticeti Vertex.

// use std::{cmp::min, sync::Arc};
// use tokio::sync::Mutex;
// use tokio::time::{sleep, Duration};

// use crate::{
//     runtime::{self, timestamp_utc},
// };
// use pevm::api::{PevmAPI, APIError};

// pub struct Scheduler {
//     // This struct can hold any necessary state for fetching scheduled tasks.
// }

// impl Scheduler {
//     pub fn new() -> Self {
//         Self {}
//     }

//     pub fn start(pevm_api: Arc<Mutex<PevmAPI>>) {
//         tracing::info!("Starting ScheduleFetcher");
//         tokio::spawn(async move {
//             Self{}.run(pevm_api).await;
//         });
//     }

//     pub async fn run(self, pevm_api: Arc<Mutex<PevmAPI>>) {
//         loop {
//             // Periodically schedule transactions
//             sleep(Duration::from_millis(100)).await;
//             let pevm_api_clone = pevm_api.clone();
//             tokio::spawn(async move {
//                 let mut guard = pevm_api_clone.lock().await;
//                 guard.schedule().await;
//             });
//         }
//     }
// }