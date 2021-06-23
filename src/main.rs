use std::collections::HashMap;

use std::io::{self, Stdout};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode, KeyEvent};
use mqttrs::{decode_slice, encode_slice, Connack, Packet, Publish};
use tokio::io::BufReader;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{Mutex, RwLock};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph};
use tui::Terminal;

#[derive(Debug)]
struct Session {
    addr: SocketAddr,
    is_on: bool,
    tx: Sender<[u8; 1024]>,
}

impl Session {
    fn new(addr: SocketAddr, tx: Sender<[u8; 1024]>) -> Self {
        Session {
            addr,
            is_on: false,
            tx,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("192.168.0.108:1883").await?;

    let state: HashMap<SocketAddr, Session> = HashMap::new();
    let state_ref = Arc::new(Mutex::new(state));

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

        let mut session = state.lock().await;
        let (tx, rx) = channel(1);
        session.insert(addr, Session::new(addr, tx));

        let state = state.clone();
        tokio::spawn(async move {
            client_worker(socket, rx).await;
        });
    }
}

async fn tui_worker(
    state: Arc<Mutex<HashMap<SocketAddr, Session>>>,
    mut terminal: Terminal<CrosstermBackend<Stdout>>,
) -> () {
    terminal.clear().unwrap();
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    loop {
        let store = state.lock().await;
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
                                store.iter().nth(list_state.selected().unwrap())
                            {
                                let mut buf = [0u8; 1024];
                                let topic = format!("cmnd/{}/POWER", id);
                                let command = mqttrs::Packet::Publish(Publish {
                                    dup: false,
                                    qospid: mqttrs::QosPid::AtMostOnce,
                                    retain: false,
                                    topic_name: &topic,
                                    payload: "ON".as_bytes(),
                                });
                                encode_slice(&command, &mut buf);
                                session.tx.send(buf).await;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(false) => {}
            Err(err) => panic!(err),
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

async fn reply<'a>(socket: &mut tokio::net::TcpStream, packet: &Packet<'a>, buf: &mut [u8; 1024]) {
    match encode_slice(packet, buf) {
        Ok(size) => {
            socket.write(&buf[..size]).await.unwrap();
        }
        Err(error) => println!("{}", error),
    }
}

async fn client_worker(mut socket: TcpStream, mut rx: Receiver<([u8; 1024], usize)>) {
    let mut buf = [0u8; 1024];
    let (mut rd, mut wrt) = socket.split();

    tokio::spawn(async move {
        loop {
            if let Some((buf, size)) = rx.recv().await {
                wrt.write(&buf[..size]).await;
            }
        }
    });

    loop {
        rd.read(&mut buf).await.unwrap();
        match decode_slice(&buf) {
            Ok(Some(pkt)) => match pkt {
                mqttrs::Packet::Connect(pkt) => {
                    let connack = Packet::Connack(Connack {
                        session_present: false,
                        code: mqttrs::ConnectReturnCode::Accepted,
                    });
                    reply(&mut socket, &connack, &mut buf).await;
                }
                Packet::Publish(pkt) => {
                    // println!(
                    //     "[{}]: {} [{:?}]",
                    //     pkt.topic_name,
                    //     String::from_utf8_lossy(&pkt.payload),
                    //     pkt.payload,
                    // )
                }
                Packet::Pingreq => {
                    reply(&mut socket, &Packet::Pingresp, &mut buf).await;
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
