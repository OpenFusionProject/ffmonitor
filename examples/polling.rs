use std::{thread, time::Duration};

use ffmonitor::Monitor;
use log::LevelFilter;

fn main() {
    env_logger::builder()
        .format_timestamp(None)
        .filter_level(LevelFilter::max())
        .init();

    let address = "127.0.0.1:8003";
    println!("Connecting to monitor at {}", address);
    let mut monitor = Monitor::new(address).expect("Bad address");
    let mut last_connected_state = monitor.is_connected();
    loop {
        let connected = monitor.is_connected();
        if connected != last_connected_state {
            last_connected_state = connected;
            if connected {
                println!("Connected to monitor");
            } else {
                println!("Monitor disconnected");
            }
        }

        while let Some(update) = monitor.poll() {
            println!("Player count: {}", update.get_player_count());
            let events = update.get_events();
            if events.is_empty() {
                println!("\tNo events");
            } else {
                for event in events {
                    println!("\t{:?}", event);
                }
            }
        }

        // wait for next updates
        thread::sleep(Duration::from_millis(500));
    }
}
