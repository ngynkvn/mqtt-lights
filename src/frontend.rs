use crate::Session;
use std::collections::HashMap;
use std::io::Stdout;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode, KeyEvent};
use mqttrs::{decode_slice, encode_slice, Connack, Packet, Publish};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{Mutex, RwLock};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph};
use tui::Terminal;

pub async fn tui_worker(
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
                rect.render_widget(Paragraph::new("Hi!"), chunks[1]);
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
                                encode_slice(&command, &mut buf).unwrap();
                                session.tx.send(buf).await;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(false) => {}
            Err(err) => panic!("{}", err),
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
