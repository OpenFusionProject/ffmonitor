use std::{thread, time::Duration};

use ffmonitor::{Monitor, MonitorNotification};
use log::LevelFilter;

fn callback(notifcation: MonitorNotification) {
    match notifcation {
        MonitorNotification::Connected => {
            println!("Connected to monitor");
        }
        MonitorNotification::Disconnected => {
            println!("Monitor disconnected");
        }
        MonitorNotification::Updated(update) => {
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
    }
}

fn main() {
    env_logger::builder()
        .format_timestamp(None)
        .filter_level(LevelFilter::max())
        .init();

    let address = "127.0.0.1:8003";
    println!("Connecting to monitor at {}", address);
    let _monitor = Monitor::new_with_callback(address, Box::new(callback)).expect("Bad address");
    loop {
        thread::sleep(Duration::from_millis(500));
    }
}
