# ffmonitor

Extremely lightweight Rust crate for reading events off an OpenFusion monitor port. 

## Usage

```
let mut monitor = Monitor::new(address).expect("Bad address");
if let Some(update) = monitor.poll() {
    println!("Player count: {}", update.get_player_count());
    let events = update.get_events();
    if events.is_empty() {
        println!("\tNo events");
    } else {
        for event in events {
            match event {
                Event::Player(playerInfo) => ...,
                Event::Chat(chat) => ...,
                ...
            }
        }
    }
} else {
    // monitor update not ready yet
    thread::sleep(Duration::from_millis(500));
}
```

See `examples/monitor.rs` for a more in-depth example.
