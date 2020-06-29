use std::sync::{Arc,RwLock};
use std::collections::HashMap;
use futures::prelude::*;
use pk_common::*;
use crate::server::Server;
use pk_common::piece_table::PieceTable;
use crate::buffer::Buffer;
use crate::config::Config;
use super::Error;

pub enum UserMessageType {
    Error, Warning, Info
}

type UserMessageActions = (Vec<String>, Box<dyn Fn(usize, PEditorState) + Send + Sync>); 

pub struct UserMessage {
    pub mtype: UserMessageType,
    pub message: String,
    pub actions: Option<UserMessageActions>,
    ttl: f32
}

const USER_MESSAGE_TTL: f32 = 3.0f32;

impl UserMessage {
    pub fn error(message: String, actions: Option<UserMessageActions>) -> UserMessage {
        UserMessage {
            mtype: UserMessageType::Error,
            message, actions,
            ttl: USER_MESSAGE_TTL 
        }
    }

    pub fn warning(message: String, actions: Option<UserMessageActions>) -> UserMessage {
        UserMessage {
            mtype: UserMessageType::Warning,
            message, actions,
            ttl: USER_MESSAGE_TTL 
        }
    }
 
    pub fn info(message: String, actions: Option<UserMessageActions>) -> UserMessage {
        UserMessage {
            mtype: UserMessageType::Info,
            message, actions,
            ttl: USER_MESSAGE_TTL 
        }
    }
}


pub enum PaneContent {
    Empty,
    Buffer {
        buffer_index: usize,
    }
}

use runic::Rect;

pub struct Pane {
    pub content: PaneContent,

    // in units of 0..1 where 0 is the top/left edge of the screen and 1 is the bottom/right edge
    pub bounds: Rect,

    // [ left, right, top, bottom ]
    pub neighbors: [Option<usize>; 4]
}

fn split_rect(rect: Rect, dir: bool, size: f32) -> (Rect, Rect) {
    let inv_size = 1.0 - size;
    if dir {
        (Rect::xywh(rect.x, rect.y, rect.w * inv_size, rect.h),
         Rect::xywh(rect.x + rect.w*inv_size, rect.y, rect.w * size, rect.h))
    } else {
        (Rect::xywh(rect.x, rect.y, rect.w, rect.h*inv_size),
         Rect::xywh(rect.x, rect.y + rect.h*inv_size, rect.w, rect.h*size))
    }
}

impl Pane {
    pub fn whole_screen(content: PaneContent) -> Pane {
        Pane {
            content,
            bounds: Rect::xywh(0.0, 0.0, 1.0, 1.0),
            neighbors: [None, None, None, None]
        }
    }

    // split always places `new_content to the right and below `index`
    pub fn split(panes: &mut Vec<Pane>, index: usize, direction: bool, size: f32, new_content: PaneContent) -> usize {
        let ix = panes.len();
        let this = &mut panes[index];
        let (a, b) = split_rect(this.bounds, direction, size);
        this.bounds = a;
        let nb = if direction {
            let n = this.neighbors[1];
            this.neighbors[1] = Some(ix);
            [Some(index), n, this.neighbors[2], this.neighbors[3]]
        } else {
            let n = this.neighbors[2];
            this.neighbors[3] = Some(ix);
            [this.neighbors[0], this.neighbors[1], Some(index), n]
        };
        panes.push(Pane {
            content: new_content,
            bounds: b,
            neighbors: nb
        });
        ix
    }
}


pub struct EditorState {
    pub buffers: Vec<Buffer>, 
    pub current_buffer: usize,
    pub panes: Vec<Pane>,
    pub current_pane: usize,
    pub registers: HashMap<char, String>,
    pub command_line: Option<(usize, PieceTable)>,
    pub thread_pool: futures::executor::ThreadPool,
    pub servers: HashMap<String, Server>,
    pub force_redraw: bool,
    pub usrmsgs: Vec<UserMessage>,
    pub selected_usrmsg: usize,
    pub config: Config
}

impl Default for EditorState {
    fn default() -> EditorState {
        EditorState::with_config(Config::default())
    }
}

pub type PEditorState = Arc<RwLock<EditorState>>;

impl EditorState {
    pub fn with_config(config: Config) -> EditorState {
        use futures::executor::ThreadPoolBuilder;
        EditorState {
            buffers: Vec::new(),
            current_buffer: 0,
            panes: Vec::new(),
            current_pane: 0,
            registers: HashMap::new(),
            command_line: None,
            thread_pool: ThreadPoolBuilder::new().create().unwrap(),
            servers: HashMap::new(),
            force_redraw: false,
            usrmsgs: Vec::new(),
            selected_usrmsg: 0,
            config
        }
    }

    pub fn current_pane(&self) -> &Pane {
        &self.panes[self.current_pane]
    }

    pub fn current_pane_mut(&mut self) -> &mut Pane {
        &mut self.panes[self.current_pane]
    }

    pub fn current_buffer(&self) -> Option<&Buffer> {
        match self.current_pane().content {
            PaneContent::Buffer { buffer_index: ix, .. } => {
                Some(&self.buffers[ix])
            },
            _ => None
        }
    }

    pub fn connect_to_server(state: PEditorState, name: String, url: &str) {
        let tp = {state.read().unwrap().thread_pool.clone()};
        let stp = tp.clone();
        let url = url.to_owned();
        tp.spawn_ok(async move {
                    let mut state = state.write().unwrap();
            match Server::init(&url, stp) {
                Ok(s) => {
                    println!("c {:?}", std::time::Instant::now());
                    state.servers.insert(name.clone(), s);
                    EditorState::process_usr_msg(&mut state, UserMessage::info(
                            format!("Connected to {} ({})!", name, url),
                            None));
                }
                Err(e) => {
                    EditorState::process_usr_msg(&mut state,
                        UserMessage::error(
                            format!("Connecting to {} ({}) failed (reason: {}), retry?", name, url, e),
                                Some((vec!["Retry".into()], Box::new(move |_, sstate| {
                                    EditorState::connect_to_server(sstate, name.clone(), &url);
                                })))
                            ));
                }
            }
        });
    }

    pub fn make_request_async<F>(state: PEditorState, server_name: impl AsRef<str>, request: protocol::Request, f: F)
        where F: FnOnce(PEditorState, protocol::Response) + Send + Sync + 'static
    {
        let tp = {state.read().unwrap().thread_pool.clone()};
        let req_fut = match {
            println!("a {:?}", std::time::Instant::now());
            state.write().unwrap().servers.get_mut(server_name.as_ref())
                .ok_or(Error::InvalidCommand(String::from("server name ") + server_name.as_ref() + " is unknown"))
        } {
            Ok(r) => r.request(request),
            Err(e) => {
                state.write().unwrap().process_error(e);
                return;
            }
        };
        let ess = state.clone();
        tp.spawn_ok(req_fut.then(move |resp: protocol::Response| async move
        {
            match resp {
                protocol::Response::Error { message } => {
                    ess.write().unwrap().process_error_str(message);
                },
                _ => f(ess, resp)
            }
        }));
    }

    pub fn sync_buffer(state: PEditorState, buffer_index: usize) {
        let (server_name, id, new_text, version) = {
            let state = state.read().unwrap();
            let b = &state.buffers[buffer_index];
            if b.currently_in_conflict { return; }
            (b.server_name.clone(), b.file_id, b.text.text(), b.version+1)
        };
        EditorState::make_request_async(state, server_name,
            protocol::Request::SyncFile { id, new_text, version },
            move |ess, resp| {
                match resp {
                    protocol::Response::Ack => {
                        let mut state = ess.write().unwrap();
                        state.buffers[buffer_index].version = version;
                    },
                    protocol::Response::VersionConflict { id, client_version_recieved: _,
                        server_version, server_text } =>
                    {
                        // TODO: probably need to show a nice little dialog, ask the user what they
                        // want to do about the conflict. this becomes a tricky situation since
                        // there's no reason to become Git, but it is nice to able to handle this
                        // situation in a nice way
                        let mut state = ess.write().unwrap();
                        let b = &mut state.buffers[buffer_index];
                        b.currently_in_conflict = true;
                        let m = format!("Server version of {}:{} conflicts with local version!",
                                        b.server_name, b.path.to_str().unwrap_or(""));
                        state.usrmsgs.push(UserMessage::warning(m,
                                Some((vec![
                                      "Keep local version".into(),
                                      "Open server version/Discard local".into(),
                                      "Open server version in new buffer".into()
                                ], Box::new(move |index, state| {
                                    let mut state = state.write().unwrap();
                                    match index {
                                        0 => {
                                            // next time we sync, overwrite server version
                                            state.buffers[buffer_index].version = 
                                                server_version;
                                            state.buffers[buffer_index].currently_in_conflict = false;
                                        },
                                        1 => {
                                            state.buffers[buffer_index].version =
                                                server_version;
                                            state.buffers[buffer_index].text =
                                                PieceTable::with_text(&server_text);
                                            state.buffers[buffer_index].currently_in_conflict = false;
                                        },
                                        2 => {
                                            state.current_buffer = state.buffers.len();
                                            let p = state.buffers[buffer_index].path.clone();
                                            let f = state.buffers[buffer_index].format.clone();
                                            let server_name = state.buffers[buffer_index].server_name.clone();
                                            state.buffers.push(Buffer::from_server(server_name, p,
                                                    id, server_text.clone(), server_version, f));
                                            // don't clear conflict flag on buffer so we don't try
                                            // to sync the conflicting version again. TODO: some
                                            // way to manually clear the flag?
                                        },
                                        _ => {} 
                                    }
                                })))
                        ));
                    }
                    _ => panic!() 
                }
            }
        );
    }

    pub fn process_usr_msg(&mut self, um: UserMessage) {
        self.usrmsgs.push(um);
        self.force_redraw = true;
    }
    
    pub fn process_usr_msgp(state: PEditorState, um: UserMessage) {
        state.write().unwrap().process_usr_msg(um);
    }

    pub fn process_error_str(&mut self, e: String) {
        self.process_usr_msg(UserMessage::error(e, None));
    }
    pub fn process_error<E: std::error::Error>(&mut self, e: E) {
        self.process_error_str(format!("{}", e));
    }
}

pub struct AutosyncWorker {
    state: PEditorState,
    last_synced_action_ids: HashMap<String, HashMap<protocol::FileId, usize>> 
}

impl AutosyncWorker {
    pub fn new(state: PEditorState) -> AutosyncWorker {
        AutosyncWorker { state, last_synced_action_ids: HashMap::new() }
    }

    pub fn run(&mut self) {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            // should this function directly manipulate the futures? 
            // it would be possible to join all the request futures together and then poll them
            // with only one task, which would be more efficent.
            let mut need_sync = Vec::new();
            {
            let state = self.state.read().unwrap();
            for (i,b) in state.buffers.iter().enumerate() {
                if let Some(last_synced_action_id) = self.last_synced_action_ids
                    .entry(b.server_name.clone())
                        .or_insert_with(HashMap::new)
                    .insert(b.file_id, b.text.most_recent_action_id())
                {
                    if last_synced_action_id < b.text.most_recent_action_id() {
                        need_sync.push(i);
                    }
                }
            }
            }
            // println!("autosync {:?}", need_sync);
            for i in need_sync {
                EditorState::sync_buffer(self.state.clone(), i);
            }
        }
    }
}



