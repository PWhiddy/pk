
use runic::*;
use pk_common::piece_table::PieceTable;
use crate::mode::CursorStyle;
use crate::config::Config;

trait CursorStyleDraw {
    fn paint(&self, rx: &mut RenderContext, char_bounds: &Rect, em_bounds: &Rect);
}

impl CursorStyleDraw for CursorStyle {
    fn paint(&self, rx: &mut RenderContext, char_bounds: &Rect, em_bounds: &Rect) {
        match self {
            CursorStyle::Line => {
                rx.fill_rect(Rect::xywh(char_bounds.x-1.0, char_bounds.y, 2.0, char_bounds.h.max(em_bounds.h)));
            },
            CursorStyle::Block => {
                rx.fill_rect(Rect::xywh(char_bounds.x, char_bounds.y, char_bounds.w.max(em_bounds.w), char_bounds.h.max(em_bounds.h)));
            },
            CursorStyle::Box => {
                rx.stroke_rect(*char_bounds, 1.0);
            },
            CursorStyle::Underline => {
                rx.fill_rect(Rect::xywh(char_bounds.x, char_bounds.y+char_bounds.h-2.0, char_bounds.w.max(em_bounds.w), 2.0));
            },
        }
    }
}

pub struct PieceTableRenderer {
    fnt: Font,
    pub em_bounds: Rect,
    pub viewport_start: usize,
    pub cursor_style: CursorStyle
}

impl PieceTableRenderer {
    pub fn init(rx: &mut RenderContext, fnt: Font) -> Self {
        let ml = rx.new_text_layout("M", &fnt, 100.0, 100.0).expect("create em size layout");
        PieceTableRenderer { fnt, viewport_start: 0, em_bounds: ml.bounds(), cursor_style: CursorStyle::Underline }
    }

    fn viewport_end(&self, bounds: &Rect) -> usize {
        self.viewport_start + ((bounds.h / self.em_bounds.h).floor() as usize).saturating_sub(2)
    }

    pub fn ensure_line_visible(&mut self, line: usize, bounds: Rect) {
        let viewport_end = self.viewport_end(&bounds);
        if self.viewport_start >= line { self.viewport_start = line.saturating_sub(1); }
        if viewport_end <= line { self.viewport_start += line - viewport_end; }
    }

    pub fn paint(&mut self, rx: &mut RenderContext, table: &PieceTable, cursor_index: usize, config: &Config, bounds: Rect) {
        rx.set_color(config.colors.foreground);
        let mut global_index = 0usize;
        let mut cur_pos = Point::xy(bounds.x, bounds.y); 
        let mut line_num = 0usize;
        let viewport_end = self.viewport_end(&bounds);
        for p in table.pieces.iter() {
            let src = &table.sources[p.source][p.start..(p.start+p.length)];
            let mut lni = src.split('\n').peekable(); 
            loop {
                let ln = lni.next();
                if ln.is_none() { break; }
                let ln = ln.unwrap();
                if line_num < self.viewport_start {
                    if lni.peek().is_some() { line_num+=1; }
                    global_index += ln.len()+1;
                    continue;
                }
                let layout = rx.new_text_layout(ln, &self.fnt, 10000.0, 10000.0).expect("create text layout");
                rx.draw_text_layout(cur_pos, &layout);
                if cursor_index >= global_index && cursor_index <= global_index+ln.len() {
                    let curbounds = layout.char_bounds(cursor_index - global_index).offset(cur_pos);
                    self.cursor_style.paint(rx, &curbounds, &self.em_bounds);
                }
                let text_size = layout.bounds();
                cur_pos.x += text_size.w; 
                global_index += ln.len();
                if lni.peek().is_some() {
                    // new line
                    line_num+=1;
                    cur_pos.x = bounds.x;
                    cur_pos.y += text_size.h.min(self.em_bounds.h);
                    global_index += 1;
                    if line_num > viewport_end { break; }
                    //if cur_pos.y + text_size.h > bounds.h { break; }
                } else {
                    break;
                }
            }
        }
    }
}
