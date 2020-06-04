#![allow(dead_code)]

#[macro_use]
extern crate lazy_static;

mod piece_table_render;

use runic::*;
use pk_common::*;
use pk_common::piece_table;
use pk_common::command::*;
use pk_common::mode::*;
use piece_table_render::PieceTableRenderer;
use std::collections::HashMap;


struct Server {
    name: String,
    socket: nng::Socket
}

struct PkApp {
    fnt: Font, buf: Buffer, registers: HashMap<char, String>,
    txr: PieceTableRenderer,
    mode: Box<dyn Mode>, last_err: Option<Error>
}

impl runic::App for PkApp {
    fn init(rx: &mut RenderContext) -> Self {
        let fnt = rx.new_font("Fira Code", 14.0, FontWeight::Regular, FontStyle::Normal).unwrap();
        let txr = PieceTableRenderer::init(rx, fnt.clone());
        PkApp {
            fnt, txr, buf: Buffer::default(),//from_file(&std::path::Path::new("pk-runic-client/src/main.rs")).unwrap(),
            mode: Box::new(NormalMode::new()), last_err: None, registers: HashMap::new()
        }
    }

    fn event(&mut self, e: runic::Event) -> bool {
        if let Event::KeyboardInput { input: KeyboardInput { state: ElementState::Pressed, .. }, .. } = e {
            self.last_err = None;
        }
        match e {
            Event::CloseRequested => return true,
            _ => {
                match self.mode.event(e, &mut self.buf, &mut self.registers) {
                    Ok(Some(new_mode)) => { self.mode = new_mode },
                    Ok(None) => {},
                    Err(e) => self.last_err = Some(e)
                };
            }
        }
        false
    }

    fn paint(&mut self, rx: &mut RenderContext) {
        rx.clear(Color::black());
        if let Some(e) = &self.last_err {
            rx.set_color(Color::rgb(0.9, 0.1, 0.0));
            rx.draw_text(Rect::xywh(4.0, rx.bounds().h - 16.0, 1000.0, 1000.0), &format!("error: {}", e), &self.fnt);
        }
        rx.set_color(Color::rgb(0.7, 0.35, 0.0));
        rx.draw_text(Rect::xywh(8.0, 2.0, 1000.0, 100.0),
            &format!("{} col {} {}@last{}-start{}-next{}", self.mode, self.buf.current_column(),
                self.buf.cursor_index, self.buf.last_line_index(self.buf.cursor_index),
                self.buf.current_start_of_line(self.buf.cursor_index), self.buf.next_line_index(self.buf.cursor_index)), &self.fnt);
        self.txr.cursor_index = self.buf.cursor_index;
        self.txr.cursor_style = self.mode.cursor_style();
        self.txr.paint(rx, &self.buf.text, Rect::xywh(8.0, 20.0, rx.bounds().w-8.0, rx.bounds().h-20.0));

        let mut y = 30.0;
        let mut global_index = 0;
        for p in self.buf.text.pieces.iter() {
            rx.draw_text(Rect::xywh(rx.bounds().w / 2.0, y, 1000.0, 1000.0), &format!("{}| \"{}\"", global_index, 
                                                                        &self.buf.text.sources[p.source][p.start..p.start+p.length].escape_debug()), &self.fnt);
            global_index += p.length;
            y += 16.0;
        }
    }
}

fn main() {
    runic::start::<PkApp>(WindowOptions::new().with_title("pk"))
}
