use super::*;
use crate::buffer::Buffer;
 
pub trait CommandFn {
    fn process(&self, es: PEditorState, a: &regex::Captures) -> mode::ModeEventResult;
}

pub struct TestCommand;

impl CommandFn for TestCommand {
    fn process(&self, editor_state: PEditorState, args: &regex::Captures) -> mode::ModeEventResult {
        println!("args = {:?}", args.get(1));
        EditorState::process_usr_msgp(editor_state,
            UserMessage::info("This is a test".into(),
                              Some((vec!["option 1".into(), "looong option 2".into(), "option 3".into()],
                              Box::new(move |index, _state| {
                                  println!("selected {}", index);
                              })))));
        Ok(Some(Box::new(NormalMode::new())))
    }
}

pub struct EditFileCommand;

impl CommandFn for EditFileCommand {
    fn process(&self, es: PEditorState, a: &regex::Captures) -> mode::ModeEventResult {
        use std::path::PathBuf;
        let server_name: String = a.name("server_name").map(|m| m.as_str()).unwrap_or("local").to_owned();
        let path = a.name("path").map(|m| PathBuf::from(m.as_str()))
            .ok_or(Error::InvalidCommand("missing path for editing a file".into()))?;
        EditorState::make_request_async(es, server_name.clone(), protocol::Request::OpenFile { path: path.clone() }, |ess, resp| {
            match resp {
                protocol::Response::FileInfo { id, contents, version } => {
                    let mut state = ess.write().unwrap();
                    state.current_buffer = state.buffers.len();
                    state.buffers.push(Buffer::from_server(String::from(server_name), path, id, contents, version));
                    state.force_redraw = true;
                },
                _ => panic!() 
            }
        });
        Ok(Some(Box::new(NormalMode::new())))
    }
}

pub struct SyncFileCommand;

impl CommandFn for SyncFileCommand {
    fn process(&self, es: PEditorState, a: &regex::Captures) -> mode::ModeEventResult {
        let cb = { es.read().unwrap().current_buffer };
        EditorState::sync_buffer(es, cb);
        Ok(Some(Box::new(NormalMode::new())))
    }
}

pub struct ConnectToServerCommand;

impl CommandFn for ConnectToServerCommand {
    fn process(&self, es: PEditorState, args: &regex::Captures) -> mode::ModeEventResult {
        println!("connect {:?}", args);
        EditorState::connect_to_server(es,
               args.name("server_name")
                    .ok_or_else(|| Error::InvalidCommand("expected server name for new connection".into()))?.as_str().into(),
               args.name("server_url")
                    .ok_or_else(|| Error::InvalidCommand("expected server URL for new connection".into()))?.as_str().into());
        Ok(Some(Box::new(NormalMode::new())))
    }
}

