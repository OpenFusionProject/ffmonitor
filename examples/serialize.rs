use ffmonitor::{
    BroadcastEvent, BroadcastScope, ChatEvent, ChatKind, EmailEvent, Event, MonitorUpdate,
    NameRequestEvent, PlayerEvent,
};

fn main() {
    let mut monitor_update = MonitorUpdate::default();

    // Player event
    monitor_update.add_event(Event::Player(PlayerEvent {
        x_coord: 10,
        y_coord: -20,
        name: "Captain Courage".to_string(),
    }));

    // Chat event
    monitor_update.add_event(Event::Chat(ChatEvent {
        kind: ChatKind::FreeChat,
        from: "Captain Courage".to_string(),
        to: None,
        message: "Hello world!".to_string(),
    }));

    monitor_update.add_event(Event::Chat(ChatEvent {
        kind: ChatKind::BuddyChat,
        from: "Captain Courage".to_string(),
        to: Some("Corporal Cautious".to_string()),
        message: "Hello friend!".to_string(),
    }));

    // Broadcast event
    monitor_update.add_event(Event::Broadcast(BroadcastEvent {
        scope: BroadcastScope::Local,
        announcement_type: 1,
        duration_secs: 5,
        from: "Captain Courage".to_string(),
        message: "Brace for impact!".to_string(),
    }));

    // Email event
    monitor_update.add_event(Event::Email(EmailEvent {
        from: "Captain Courage".to_string(),
        to: "Corporal Cautious".to_string(),
        subject: Some("Secret mission".to_string()),
        body: vec![
            "We need to infiltrate the fusion lair.".to_string(),
            "Meet me at the rendezvous point.".to_string(),
            "".to_string(),
            "-Captain Courage".to_string(),
        ],
    }));

    monitor_update.add_event(Event::Email(EmailEvent {
        from: "Corporal Cautious".to_string(),
        to: "Captain Courage".to_string(),
        subject: None,
        body: vec!["Roger that.".to_string()],
    }));

    // Name request event
    monitor_update.add_event(Event::NameRequest(NameRequestEvent {
        player_uid: 123,
        requested_name: "Colonel Catastrophe".to_string(),
    }));

    println!("{}", monitor_update);
}
