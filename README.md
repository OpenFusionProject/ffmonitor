# ffmonitor

Extremely lightweight Rust crate for reading events off an OpenFusion monitor port.

## About

`ffmonitor` provides a `Monitor` object that spins up a background thread to parse tokens from an OpenFusion monitor port. There are two modes of operation:
- The `Monitor` buffers monitor updates in memory that can be retrieved using `Monitor::poll()`. (default behavior)
- The `Monitor` does not buffer updates and instead sends them to a user-provided callback.

Supported events:
- Player position events (`player`)
- Player chat events (`chat`)
- Email events (`email`)

## Usage

Polling mode:
```rust
let mut monitor = Monitor::new("127.0.0.1:8003").unwrap();
loop {
    while let Some(update) = monitor.poll() {
        println!("Player count: {}", update.get_player_count());
        let events = update.get_events();
        if events.is_empty() {
            println!("No events");
        } else {
            for event in events {
                match event {
                    Event::Player(playerInfo) => ...,
                    Event::Chat(chat) => ...,
                    ...
                }
            }
        }
    }

    // wait for next updates
    thread::sleep(Duration::from_millis(500));
}
```

Callback mode:
```rust
fn callback(notifcation: MonitorNotification) {
    if let MonitorNotification::Updated(update) = notification {
        println!("Player count: {}", update.get_player_count());
        let events = update.get_events();
        if events.is_empty() {
            println!("No events");
        } else {
            for event in events {
                match event {
                    Event::Player(playerInfo) => ...,
                    Event::Chat(chat) => ...,
                    ...
                }
            }
        }
    }
}

let _monitor = Monitor::new_with_callback("127.0.0.1", Box::new(callback)).unwrap();
loop {
    // do something else with your life
    thread::sleep(Duration::from_millis(500));
}
```

See the examples for more detail.
