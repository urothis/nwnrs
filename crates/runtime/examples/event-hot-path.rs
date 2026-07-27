//! Release-mode budget check for the subscription bitset hot path.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::{hint::black_box, time::Instant};

use nwnrs_runtime::{EVENT_CATALOG, EventSubscriptionMask, keyed_event_definition};

const ITERATIONS: usize = 20_000_000;
const ITERATIONS_U32: u32 = 20_000_000;
const MAXIMUM_AVERAGE_NS: f64 = 500.0;

fn main() {
    let mut subscriptions = EventSubscriptionMask::empty();
    let mut keys = Vec::with_capacity(EVENT_CATALOG.len());
    for event in EVENT_CATALOG {
        let Some((key, _definition)) = keyed_event_definition(event.subject, event.phase) else {
            eprintln!(
                "catalog event {}::{} has no stable key",
                event.subject, event.phase
            );
            std::process::exit(1);
        };
        let _inserted = subscriptions.insert(key);
        keys.push(key);
    }
    if keys.is_empty() {
        eprintln!("event catalog is empty");
        std::process::exit(1);
    }
    let started = Instant::now();
    let mut matches = 0_usize;
    for key in keys.iter().copied().cycle().take(ITERATIONS) {
        matches += usize::from(black_box(&subscriptions).contains(black_box(key)));
    }
    let elapsed = started.elapsed();
    black_box(matches);
    let average_ns = elapsed.as_nanos() as f64 / f64::from(ITERATIONS_U32);
    println!(
        "event subscription hot path: {ITERATIONS} lookups in {elapsed:?}, average \
         {average_ns:.3}ns"
    );
    if average_ns > MAXIMUM_AVERAGE_NS {
        eprintln!(
            "event subscription hot path exceeded the {MAXIMUM_AVERAGE_NS:.0}ns average budget"
        );
        std::process::exit(1);
    }
}
