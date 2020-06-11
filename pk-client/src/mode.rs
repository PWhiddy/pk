
use std::fmt;
use std::collections::HashMap;
use runic::*;
use super::*;
use std::sync::{Arc,RwLock};

pub enum CursorStyle {
    Line, Block, Box, Underline
}

pub type ModeEventResult = Result<Option<Box<dyn Mode>>, Error>;

pub trait Mode : fmt::Display {
    fn event(&mut self, e: Event, state: PEditorState) -> ModeEventResult;
    fn cursor_style(&self) -> CursorStyle { CursorStyle::Block }
}

pub struct NormalMode {
    pending_buf: String
}

impl NormalMode {
    pub fn new() -> NormalMode {
        NormalMode { pending_buf: String::new() }
    }
}

impl fmt::Display for NormalMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "normal [{}]", self.pending_buf)
    }
}

impl Mode for NormalMode {
    fn event(&mut self, e: Event, state: PEditorState) -> ModeEventResult {
        match e {
            Event::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(vk), .. }, .. } => {
                match vk {
                    VirtualKeyCode::Escape => {
                        self.pending_buf.clear();
                        Ok(None)
                    },
                    _ => Ok(None) 
                }
            },
            Event::ReceivedCharacter(c) if !c.is_control() => {
                use super::command::*;
                self.pending_buf.push(c);
                match Command::parse(&self.pending_buf) {
                    Ok(cmd) => {
                        let res = {
                            match cmd.execute(&mut state.write().unwrap()) {
                            Ok(r) => r,
                            Err(e) => {
                                self.pending_buf.clear();
                                return Err(e);
                            }
                        } 
                        };
                        self.pending_buf.clear();
                        match res {
                            None | Some(ModeTag::Normal) => Ok(None),
                            Some(ModeTag::Command) => Ok(Some(Box::new(CommandMode::new(state)))),
                            Some(ModeTag::Insert) => {
                                let mut state = state.write().unwrap();
                                let cb = state.current_buffer;
                                let buf = &mut state.buffers[cb];
                                Ok(Some(Box::new(InsertMode {
                                    tmut: buf.text.insert_mutator(buf.cursor_index)
                                }))) 
                            },
                            _ => panic!("unknown mode: {:?}", res)
                        }
                    },
                    Err(Error::IncompleteCommand) => Ok(None),
                    Err(e) => { 
                        self.pending_buf.clear();
                        Err(e)
                    }
                }
                /*Ok(Some(Box::new(InsertMode {
                  tmut: buf.text.insert_mutator(buf.cursor_index)
                  })))*/
            },
            _ => Ok(None)
        }
    }
}


pub struct InsertMode {
    tmut: piece_table::TableMutator
}


impl fmt::Display for InsertMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "insert")
    }
}

impl Mode for InsertMode {
    fn cursor_style(&self) -> CursorStyle { CursorStyle::Line }

    fn event(&mut self, e: Event, state: PEditorState) -> ModeEventResult {
        let mut state = state.write().unwrap();
        let cb = state.current_buffer;
        let buf = &mut state.buffers[cb];
        match e {
            Event::ReceivedCharacter(c) if !c.is_control() => {
                self.tmut.push_char(&mut buf.text, c);
                buf.cursor_index += 1;
                Ok(None)
            },
            Event::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(vk), state: ElementState::Pressed, .. }, .. } => {
                match vk {
                    VirtualKeyCode::Back => {
                        if !self.tmut.pop_char(&mut buf.text) {
                            buf.cursor_index -= 1;
                        }
                        Ok(None)
                    },
                    VirtualKeyCode::Return => {
                        self.tmut.push_char(&mut buf.text, '\n');
                        buf.cursor_index += 1;
                        Ok(None)
                    }
                    VirtualKeyCode::Escape => Ok(Some(Box::new(NormalMode::new()))),
                    _ => Ok(None)
                }
            },
            _ => Ok(None)
        }
    }
}

use piece_table::TableMutator;

struct CommandMode {
    cursor_mutator: TableMutator,
    commands: Vec<(regex::Regex, Rc<dyn line_command::CommandFn>)>
}

impl CommandMode {
    fn new(state: PEditorState) -> CommandMode {
        let mut pt = PieceTable::default();
        let cursor_mutator = pt.insert_mutator(0);
        let mut state = state.write().unwrap();
        assert!(state.command_line.is_none());
        state.command_line = Some((0, pt));
        CommandMode {
            cursor_mutator,
            commands: vec![
                (regex::Regex::new("test (.*)").unwrap(), Rc::new(line_command::TestCommand)),
                (regex::Regex::new(r#"e\s+(?:(?P<server_name>.*):)?(?P<path>.*)"#).unwrap(), Rc::new(line_command::EditFileCommand))
            ],
        }
    }
}

impl Mode for CommandMode {
    fn cursor_style(&self) -> CursorStyle {
        CursorStyle::Box
    }
    
    fn event(&mut self, e: Event, state: PEditorState) 
        -> ModeEventResult
    {
        let mut pstate = state.write().unwrap();
        if let Some((cursor_index, pending_command)) = pstate.command_line.as_mut() {
            match e {
                Event::ReceivedCharacter(c) if !c.is_control() => {
                    self.cursor_mutator.push_char(pending_command, c);
                    *cursor_index += 1;
                    Ok(None)
                },
                Event::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(vk), state: ElementState::Pressed, .. }, .. } => {
                    match vk {
                        VirtualKeyCode::Left => {
                            *cursor_index = cursor_index.saturating_sub(1);
                            self.cursor_mutator = pending_command.insert_mutator(*cursor_index);
                            Ok(None)
                        },
                        VirtualKeyCode::Right => {
                            *cursor_index = (*cursor_index+1).max(pending_command.len());
                            self.cursor_mutator = pending_command.insert_mutator(*cursor_index);
                            Ok(None)
                        },
                        VirtualKeyCode::Back => {
                            self.cursor_mutator.pop_char(pending_command);
                            *cursor_index -= 1;
                            Ok(None)
                        },
                        VirtualKeyCode::Return => {
                            use line_command::CommandFn;
                            let cmdstr = pstate.command_line.take().unwrap().1.text();
                            if let Some((cmdix, args)) = self.commands.iter().enumerate()
                                .filter_map(|(i,cmd)| cmd.0.captures(&cmdstr).map(|c| (i, c))).nth(0)
                            {
                                let cmd = self.commands[cmdix].1.clone();
                                drop(pstate);
                                cmd.process(state.clone(), &args)
                            } else {
                                Ok(Some(Box::new(NormalMode::new())))
                            }
                        }
                        VirtualKeyCode::Escape => {
                            pstate.command_line = None;
                            Ok(Some(Box::new(NormalMode::new())))
                        },
                        _ => Ok(None)
                    }
                },
                _ => Ok(None)
            }
        } else {
            Err(Error::InvalidCommand("".into()))
        }
    }
}

impl fmt::Display for CommandMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "command")
    }
}

