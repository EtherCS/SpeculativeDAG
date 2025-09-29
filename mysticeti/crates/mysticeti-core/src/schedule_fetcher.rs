// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// ScheduleFetcher is responsible for fetching scheduled tasks from the PevmAPI.
// The fetched tasks can then be proposed as a Mysticeti Vertex.

use std::{cmp::min, sync::Arc};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use crate::{
    runtime::{self, timestamp_utc},
};
use pevm::api::{PevmAPI, APIError, TransactionWithHint};

pub struct ScheduleFetcher {
    // This struct can hold any necessary state for fetching scheduled tasks.
}

impl ScheduleFetcher {
    pub fn new() -> Self {
        Self {}
    }

    pub fn start(pevm_api: Arc<Mutex<PevmAPI>>) {
        tracing::info!("Starting ScheduleFetcher");
        tokio::spawn(async move {
            Self{}.run(pevm_api).await;
        });
    }

    pub async fn run(self, pevm_api: Arc<Mutex<PevmAPI>>) {
        loop {
            // Periodically fetch scheduled tasks
            let pevm_api_clone = pevm_api.clone();
            tokio::spawn(async move {
                let mut guard = pevm_api_clone.lock().await;
                let result = guard.fetch_one_scheduled_txn().await;
                match result {
                    Ok(task) => {
                        tracing::info!("Fetched scheduled task {:?}", task);
                        // [TODO] propose task to Mysticeti Vertex
                    }
                    Err(e) => {
                        tracing::error!("Error fetching scheduled task: {}", e);
                    }
                }
            });
        }
    }

    // pub async fn try_one_fetch(self, pevm_api: Arc<Mutex<PevmAPI>>) -> Result<TransactionWithHint, APIError> {
    //     let mut guard = pevm_api.lock().await;
    //     let result = guard.fetch_one_scheduled_txn().await;
    //     match result {
    //         Ok(task) => {
    //             tracing::info!("Fetched scheduled task {:?}", &task);
    //         }
    //         Err(_) => {
    //             tracing::error!("Error fetching scheduled task: {}", &result.err().unwrap());
    //         }
    //     }
    //     result
    // }
}