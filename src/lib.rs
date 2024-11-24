use std::{
    io::BufRead as _,
    net::{SocketAddr, TcpStream},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

use log::*;

type Error = Box<dyn std::error::Error>;
type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerEvent {
    pub x_coord: i32,
    pub y_coord: i32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEvent {
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EventInternal {
    Begin,
    Player(PlayerEvent),
    Chat(ChatEvent),
    End,
    Disconnect,
}
impl TryFrom<&str> for EventInternal {
    type Error = String;
    fn try_from(value: &str) -> std::result::Result<Self, String> {
        let parts: Vec<&str> = value.split_ascii_whitespace().collect();
        if parts.is_empty() {
            return Err("Empty event".into());
        }

        match parts[0] {
            "begin" => Ok(EventInternal::Begin),
            "end" => Ok(EventInternal::End),
            "player" => {
                if parts.len() < 4 {
                    return Err("player: Not enough tokens".into());
                }
                let x_coord = parts[1].parse().map_err(|_| "player: Invalid x coord")?;
                let y_coord = parts[2].parse().map_err(|_| "player: Invalid y coord")?;
                let name = parts[3..].join(" ");
                Ok(EventInternal::Player(PlayerEvent {
                    x_coord,
                    y_coord,
                    name,
                }))
            }
            "chat" => {
                if parts.len() < 2 {
                    return Err("chat: Not enough tokens".into());
                }
                let message = parts[1..].join(" ");
                Ok(EventInternal::Chat(ChatEvent { message }))
            }
            other => Err(format!("Unknown event type: {}", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Player(PlayerEvent),
    Chat(ChatEvent),
}
impl From<EventInternal> for Event {
    fn from(event: EventInternal) -> Self {
        match event {
            EventInternal::Player(player) => Event::Player(player),
            EventInternal::Chat(chat) => Event::Chat(chat),
            _ => unreachable!(),
        }
    }
}

fn listen(addr: SocketAddr, tx: mpsc::Sender<EventInternal>) -> Result<()> {
    info!("Connecting to monitor at {}", addr);
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    loop {
        if reader.read_line(&mut line)? == 0 {
            return Err("Connection closed".into());
        }

        match EventInternal::try_from(line.trim()) {
            Ok(event) => {
                debug!("Received event: {:?}", event);
                tx.send(event.clone()).map_err(|_| "Failed to send event")?;
            }
            Err(err) => {
                warn!("Failed to parse event ({})", err);
            }
        }
        line.clear();
    }
}

#[derive(Debug, Clone)]
pub struct MonitorUpdate {
    events: Vec<Event>,
}
impl MonitorUpdate {
    /// Decompose the MonitorUpdate into a Vec of Events
    pub fn get_events(self) -> Vec<Event> {
        self.events
    }

    pub fn get_player_count(&self) -> usize {
        self.events
            .iter()
            .filter(|event| matches!(event, Event::Player(_)))
            .count()
    }
}

pub struct Monitor {
    handle: JoinHandle<()>,
    rx: Receiver<EventInternal>,
    buf: Vec<EventInternal>,
    connected: bool,
    last_update: Option<MonitorUpdate>,
}
impl Monitor {
    pub fn new(address: &str) -> Result<Self> {
        info!("ffmonitor v{}", env!("CARGO_PKG_VERSION"));
        let address: SocketAddr = address.parse()?;
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn({
            move || loop {
                if let Err(err) = listen(address, tx.clone()) {
                    error!("Monitor connection broken: {}", err);
                    tx.clone().send(EventInternal::Disconnect).unwrap();
                    thread::sleep(Duration::from_secs(1));
                }
            }
        });
        Ok(Self {
            handle,
            rx,
            buf: Vec::new(),
            connected: false,
            last_update: None,
        })
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn poll(&mut self) -> Option<MonitorUpdate> {
        // move all the events from the queue into the buffer
        while let Ok(event) = self.rx.try_recv() {
            if event == EventInternal::Disconnect {
                self.connected = false;
                return None;
            } else {
                self.connected = true;
                self.buf.push(event);
            }
        }

        // find the begin event
        let begin_pos = self
            .buf
            .iter()
            .position(|event| *event == EventInternal::Begin)?;

        // find the end event
        let end_pos = self
            .buf
            .iter()
            .position(|event| *event == EventInternal::End)?;

        if end_pos < begin_pos {
            warn!("End event found before begin event");
            self.buf.drain(..=end_pos);
            return None;
        }

        // extract the events between begin and end, remove the begin and end events
        let mut events: Vec<EventInternal> = self.buf.drain(begin_pos..=end_pos).collect();
        events.pop();
        events.remove(0);

        // convert the internal events to public events
        let events: Vec<Event> = events.into_iter().map(Event::from).collect();
        let update = MonitorUpdate { events };
        self.last_update = Some(update.clone());

        Some(update)
    }

    pub fn get_last_update(&self) -> Option<MonitorUpdate> {
        self.last_update.clone()
    }

    pub fn shutdown(self) -> Result<()> {
        self.handle.join().map_err(|_| "Monitor thread panicked")?;
        Ok(())
    }
}
