use std::collections::HashMap;

use std::io::{self, Stdout};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode, KeyEvent};
use mqttrs::{decode_slice, encode_slice, Connack, Packet, Publish};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{Mutex, RwLock};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, BorderType, Borders, List, ListItem, ListState};
use tui::Terminal;

#[derive(Debug)]
struct Session {
    addr: SocketAddr,
    socket: Arc<Mutex<TcpStream>>,
    is_on: bool,
}

impl Session {
    fn new(addr: SocketAddr, socket: Arc<Mutex<TcpStream>>) -> Self {
        Session {
            addr,
            socket,
            is_on: false,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("192.168.0.110:1883").await?;

    let state: HashMap<SocketAddr, Session> = HashMap::new();
    let state_ref = Arc::new(RwLock::new(state));

    let state = state_ref.clone();
    tokio::spawn(async move {
        let stdout = io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).unwrap();
        tui_worker(state, terminal).await;
    });

    let state = state_ref.clone();
    loop {
        let (socket, addr) = listener.accept().await?;
        let socket = Arc::new(Mutex::new(socket));

        {
            let socket = socket.clone();
            let mut session = state.write().await;
            session.insert(addr, Session::new(addr, socket));
        }

        tokio::spawn(async move {
            client_listener(socket.clone()).await;
        });
    }
}


async fn client_listener(socket: Arc<Mutex<TcpStream>>) {
    let mut buf = [0u8; 1024];
    loop {
        let mut socket = socket.lock().await;
        socket.read(&mut buf).await.unwrap();
        match decode_slice(&buf) {
            Ok(Some(pkt)) => match pkt {
                mqttrs::Packet::Connect(_pkt) => {
                    let connack = Packet::Connack(Connack {
                        session_present: false,
                        code: mqttrs::ConnectReturnCode::Accepted,
                    });
                    let r = encode_slice(&connack, &mut buf);
                    socket.write(&buf[..r.unwrap()]).await;
                }
                Packet::Publish(_pkt) => {
                    // println!(
                    //     "[{}]: {} [{:?}]",
                    //     pkt.topic_name,
                    //     String::from_utf8_lossy(&pkt.payload),
                    //     pkt.payload,
                    // )
                }
                Packet::Pingreq => {
                    let r = encode_slice(&Packet::Pingresp, &mut buf);
                    socket.write(&buf[..r.unwrap()]).await;
                }
                unknown => {
                    println!("Not handled: {:?}", unknown);
                }
            },
            Ok(None) => {
                println!("Decode error");
            }
            Err(error) => {
                dbg!(error);
            }
        }
    }
}


async fn tui_worker(
    state: Arc<RwLock<HashMap<SocketAddr, Session>>>,
    mut terminal: Terminal<CrosstermBackend<Stdout>>,
) {
    terminal.clear().unwrap();
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    loop {
        let mut store = state.write().await;
        terminal
            .draw(|rect| {
                let size = rect.size();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints(
                        [
                            Constraint::Min(5),
                            Constraint::Length(3),
                            Constraint::Length(4),
                        ]
                        .as_ref(),
                    )
                    .split(size);
                let items: Vec<_> = store
                    .iter()
                    .map(|(key, session)| ListItem::new(format!("{} ➡ {:?}", key, session)))
                    .collect();
                let list = List::new(items)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .style(Style::default().fg(Color::White))
                            .border_type(BorderType::Thick),
                    )
                    .highlight_style(
                        Style::default()
                            .bg(Color::Yellow)
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD),
                    );
                rect.render_stateful_widget(list, chunks[0], &mut list_state)
            })
            .expect("???");

        match poll(Duration::from_millis(200)) {
            Ok(true) => {
                if let Ok(Event::Key(event)) = read() {
                    match event {
                        KeyEvent {
                            code: KeyCode::Down,
                            ..
                        } => {
                            if let Some(index) = list_state.selected() {
                                let len = store.len();
                                let i = Some(if index >= len - 1 { 0 } else { index + 1 });
                                list_state.select(i);
                            }
                        }
                        KeyEvent {
                            code: KeyCode::Up, ..
                        } => {
                            if let Some(index) = list_state.selected() {
                                let len = store.len();
                                let i = Some(if index > 0 { index - 1 } else { len - 1 });
                                list_state.select(i);
                            }
                        }
                        KeyEvent {
                            code: KeyCode::Char('t'),
                            ..
                        } => {
                            if let Some((id, session)) =
                                store.iter_mut().nth(list_state.selected().unwrap())
                            {
                                let mut buf = [0u8; 1024];
                                let topic = format!("cmnd/{}/POWER", id);
                                let command = mqttrs::Packet::Publish(Publish {
                                    dup: false,
                                    qospid: mqttrs::QosPid::AtMostOnce,
                                    retain: false,
                                    topic_name: &topic,
                                    payload: (if session.is_on {"OFF"} else {"ON"}).as_bytes(),
                                });
                                let r = encode_slice(&command, &mut buf);
                                let mut s = session.socket.lock().await;
                                s.write(&buf[..r.unwrap()]).await;
                                session.is_on = !session.is_on;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(false) => {}
            Err(err) => std::panic::panic_any(err),
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}