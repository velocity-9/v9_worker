use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::model::ComponentStats;

const DEFAULT_STAT_WINDOW: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub struct StatTracker {
    stat_window: Duration,
    // Events at the back of the queue are the newest events
    event_deque: VecDeque<StatEvent>,
}

#[derive(Debug, Clone)]
struct StatEvent {
    at: Instant,
    duration_ms: u32,
    response_bytes: u32,
}

impl Default for StatTracker {
    fn default() -> Self {
        StatTracker {
            stat_window: DEFAULT_STAT_WINDOW,
            event_deque: VecDeque::new(),
        }
    }
}

impl StatTracker {
    pub fn get_component_stats(&mut self) -> ComponentStats {
        self.pop_old_events();

        let stat_window_seconds = self.stat_window.as_secs_f64();

        let hits = self.event_deque.len() as f64;

        if self.event_deque.is_empty() {
            ComponentStats {
                stat_window_seconds,

                hits,

                avg_response_bytes: 0.0,
                avg_ms_latency: 0.0,
                ms_latency_percentiles: vec![],
            }
        } else {
            let avg_response_bytes = self
                .event_deque
                .iter()
                .map(|e| f64::from(e.response_bytes))
                .sum::<f64>()
                / hits;
            let avg_ms_latency = self
                .event_deque
                .iter()
                .map(|e| f64::from(e.duration_ms))
                .sum::<f64>()
                / hits;
            ComponentStats {
                stat_window_seconds,

                hits,

                avg_response_bytes,
                avg_ms_latency,
                ms_latency_percentiles: calculate_latency_percentiles(&self.event_deque),
            }
        }
    }

    pub fn add_stat_event(&mut self, duration_ms: u32, response_bytes: u32) {
        self.event_deque.push_back(StatEvent {
            at: Instant::now(),
            duration_ms,
            response_bytes,
        });

        self.pop_old_events();
    }

    fn pop_old_events(&mut self) {
        let too_old = Instant::now() - self.stat_window;
        while self.event_deque.front().map_or(false, |e| e.at < too_old) {
            self.event_deque.pop_front();
        }
    }
}

const PERCENTILE_BUCKETS: usize = 10;

fn calculate_latency_percentiles(entries: &VecDeque<StatEvent>) -> Vec<f64> {
    let mut u32_latencies: Vec<u32> = entries.iter().map(|e| e.duration_ms).collect();
    u32_latencies.sort();

    let mut res = Vec::new();

    // Need some special logic here for dealing with a number of entries that is not a multiple of `PERCENTILE_BUCKETS`
    // To do this, each of the first `additional_items` buckets get one extra item
    // In order to account for this, each buckets starting index needs to be bumped up
    // Notice, however, that this bump is exactly `min(i, additional_items)` where `i` is the bucket #
    // https://stackoverflow.com/a/2135920/1981468
    let rough_bucket_size = u32_latencies.len() / PERCENTILE_BUCKETS;
    let additional_items = u32_latencies.len() % PERCENTILE_BUCKETS;
    for i in 0..PERCENTILE_BUCKETS {
        let starting_index = i * rough_bucket_size + i.min(additional_items);
        let next_starting_index = (i + 1) * rough_bucket_size + (i + 1).min(additional_items);

        let slice = &u32_latencies[starting_index..next_starting_index];

        // We only care about non-empty buckets
        if !slice.is_empty() {
            let total_latency: u32 = slice.iter().sum();
            res.push(f64::from(total_latency) / slice.len() as f64);
        }
    }

    res
}
