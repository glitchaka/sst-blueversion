//! Slint/FemtoVG frontend for SST Blueversion.
//!
//! The terminal owns the whole client area. The top status island is an overlay:
//! there is deliberately no full-width title bar behind it.

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    fs,
    mem::size_of,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use arboard::Clipboard;
use chrono::Local;
use crossterm::event::{
    KeyCode as CtKeyCode, KeyEvent as CtKeyEvent, KeyModifiers as CtKeyModifiers,
};
use fontdue::{Font, FontSettings, Metrics};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{
    BackendSelector, ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer,
    TimerMode,
};
use sysinfo::System;
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::{
        Dwm::*,
        Gdi::{CreateRoundRectRgn, DeleteObject},
    },
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    UI::{
        Controls::MARGINS,
        WindowsAndMessaging::{GetForegroundWindow, IsZoomed, SetWindowRgn},
    },
};

use crate::adapters::{
    persistence::{AppPaths, AppearanceConfig},
    terminal::embedded::EmbeddedSession,
};

const INITIAL_COLS: u16 = 112;
const INITIAL_ROWS: u16 = 31;
const PAD: f32 = 14.0;
const ISLAND_TOP: f32 = 6.0;
const ISLAND_HEIGHT: f32 = 34.0;
const CELL_WIDTH: f32 = 10.0;
const CELL_HEIGHT: f32 = 23.0;
const FONT_SIZE: f32 = 19.0;

const FG: Rgb = Rgb(0xDF, 0xE8, 0xEF);
const BG: Rgb = Rgb(0x11, 0x16, 0x29);
const CURSOR: Rgb = Rgb(0x69, 0xC7, 0xFF);
const SELECTION: Rgb = Rgb(0x37, 0x4B, 0x70);

const NERD_FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/JetBrainsMonoNerdFontMono-Regular.ttf"
));

slint::slint! {
    export component SstBlueWindow inherits Window {
        title: "Shell Shock Tool";
        preferred-width: 1240px;
        preferred-height: 820px;
        min-width: 520px;
        min-height: 320px;
        no-frame: true;
        resize-border-width: 7px;
        background: transparent;

        in property <image> terminal-image;
        in property <image> background-image;
        in property <float> background-image-opacity: 0.0;
        in property <length> window-corner-radius: 16px;
        in property <string> cpu-text: "CPU 0%";
        in property <string> ram-text: "RAM 0.0 GiB";
        in property <string> clock-text: "00:00";
        in property <string> date-text: "00 ---";

        callback key-input(string, bool, bool, bool);
        callback pointer-down(float, float);
        callback pointer-move(float, float);
        callback pointer-up(float, float, bool);
        callback pointer-scroll(float);
        callback close-window();

        surface := Rectangle {
            x: 0;
            y: 0;
            width: 100%;
            height: 100%;
            border-radius: root.maximized ? 0px : root.window-corner-radius;
            clip: true;
            background: transparent;

            Image {
                x: 0;
                y: 0;
                width: 100%;
                height: 100%;
                source: root.background-image;
                image-fit: cover;
                opacity: root.background-image-opacity;
            }

            Image {
                x: 0;
                y: 0;
                width: 100%;
                height: 100%;
                source: root.terminal-image;
                image-fit: fill;
            }

        terminal-focus := FocusScope {
            x: 0;
            y: 0;
            width: 100%;
            height: 100%;
            focus-on-click: true;
            focus-on-tab-navigation: false;

            key-pressed(event) => {
                root.key-input(
                    event.text,
                    event.modifiers.control,
                    event.modifiers.alt,
                    event.modifiers.shift
                );
                accept
            }

            terminal-touch := TouchArea {
                mouse-cursor: text;

                pointer-event(event) => {
                    if (event.button == PointerEventButton.left
                        && event.kind == PointerEventKind.down) {
                        terminal-focus.focus();
                        root.pointer-down(
                            self.mouse-x / 1px,
                            self.mouse-y / 1px
                        );
                    }
                    if (event.button == PointerEventButton.left
                        && event.kind == PointerEventKind.up) {
                        root.pointer-up(
                            self.mouse-x / 1px,
                            self.mouse-y / 1px,
                            false
                        );
                    }
                    if (event.button == PointerEventButton.right
                        && event.kind == PointerEventKind.up) {
                        terminal-focus.focus();
                        root.pointer-up(
                            self.mouse-x / 1px,
                            self.mouse-y / 1px,
                            true
                        );
                    }
                }

                moved => {
                    root.pointer-move(
                        self.mouse-x / 1px,
                        self.mouse-y / 1px
                    );
                }

                scroll-event(event) => {
                    root.pointer-scroll(event.delta-y / 1px);
                    accept
                }
            }
        }

        island := Rectangle {
            width: min(720px, root.width - 20px);
            height: 34px;
            x: (root.width - self.width) / 2;
            y: 6px;
            border-radius: 13px;
            background: rgba(10, 13, 20, 0.93);
            border-width: 1px;
            border-color: #2b3547;

            WindowMoveArea {
                x: 0;
                y: 0;
                width: parent.width;
                height: parent.height;

                Text {
                    x: 17px;
                    y: 0;
                    width: 70px;
                    height: parent.height;
                    text: "SST";
                    color: #dfe8ef;
                    font-family: "Segoe UI Variable";
                    font-size: 15px;
                    font-weight: 700;
                    vertical-alignment: center;
                }

                Text {
                    visible: island.width >= 670px;
                    x: 102px;
                    y: 0;
                    width: 92px;
                    height: parent.height;
                    text: root.cpu-text;
                    color: #dfe8ef;
                    font-family: "Segoe UI Variable";
                    font-size: 13px;
                    vertical-alignment: center;
                }

                Text {
                    visible: island.width >= 670px;
                    x: 205px;
                    y: 0;
                    width: 128px;
                    height: parent.height;
                    text: root.ram-text;
                    color: #dfe8ef;
                    font-family: "Segoe UI Variable";
                    font-size: 13px;
                    vertical-alignment: center;
                }

                Text {
                    x: island.width - 296px;
                    y: 0;
                    width: 62px;
                    height: parent.height;
                    text: root.clock-text;
                    color: #dfe8ef;
                    font-family: "Segoe UI Variable";
                    font-size: 13px;
                    vertical-alignment: center;
                    horizontal-alignment: center;
                }

                Text {
                    visible: island.width >= 540px;
                    x: island.width - 235px;
                    y: 0;
                    width: 86px;
                    height: parent.height;
                    text: root.date-text;
                    color: #aeb9c8;
                    font-family: "Segoe UI Variable";
                    font-size: 12px;
                    vertical-alignment: center;
                    horizontal-alignment: center;
                }

                Rectangle {
                    x: island.width - 114px;
                    y: 1px;
                    width: 38px;
                    height: island.height - 2px;
                    border-radius: 10px;
                    background: minimize-touch.pressed
                        ? rgb(36, 49, 67)
                        : minimize-touch.has-hover ? rgb(23, 35, 52) : transparent;

                    Path {
                        x: 11px;
                        y: 10px;
                        width: 16px;
                        height: 12px;
                        commands: "M 1 1 L 8 9 L 15 1";
                        stroke: #74c8f5;
                        stroke-width: 2.4px;
                        stroke-line-cap: round;
                        stroke-line-join: round;
                    }

                    minimize-touch := TouchArea {
                        mouse-cursor: pointer;
                        clicked => { root.minimized = true; }
                    }
                }

                Rectangle {
                    x: island.width - 76px;
                    y: 1px;
                    width: 38px;
                    height: island.height - 2px;
                    border-radius: 10px;
                    background: maximize-touch.pressed
                        ? rgb(36, 49, 67)
                        : maximize-touch.has-hover ? rgb(23, 35, 52) : transparent;

                    Path {
                        x: 11px;
                        y: 11px;
                        width: 16px;
                        height: 12px;
                        commands: "M 1 10 L 8 2 L 15 10";
                        stroke: #74c8f5;
                        stroke-width: 2.4px;
                        stroke-line-cap: round;
                        stroke-line-join: round;
                    }

                    maximize-touch := TouchArea {
                        mouse-cursor: pointer;
                        clicked => { root.maximized = !root.maximized; }
                    }
                }

                Rectangle {
                    x: island.width - 38px;
                    y: 1px;
                    width: 38px;
                    height: island.height - 2px;
                    border-radius: 10px;
                    background: close-touch.pressed
                        ? rgb(62, 23, 36)
                        : close-touch.has-hover ? rgb(48, 18, 28) : transparent;

                    Path {
                        x: 10px;
                        y: 8px;
                        width: 18px;
                        height: 18px;
                        commands: "M 9 1 L 9 8 M 3.3 3.7 A 7 7 0 1 0 14.7 3.7";
                        stroke: close-touch.has-hover ? #ff5d78 : #ff9fbd;
                        stroke-width: 2px;
                        stroke-line-cap: round;
                    }

                    close-touch := TouchArea {
                        mouse-cursor: pointer;
                        clicked => { root.close-window(); }
                    }
                }
            }
        }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rgb(u8, u8, u8);

type TerminalAppearance = AppearanceConfig;

fn parse_rgb(value: &str) -> Result<Rgb> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        anyhow::bail!("el color debe usar formato #RRGGBB");
    }
    let rgb = u32::from_str_radix(hex, 16)?;
    Ok(Rgb(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    ))
}

fn load_background_image(paths: &AppPaths, appearance: &TerminalAppearance) -> Result<Option<Image>> {
    let raw = appearance.background_image.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let configured = std::path::PathBuf::from(raw);
    let path = if configured.is_absolute() {
        configured
    } else {
        paths.root_dir().join(configured)
    };

    let decoded = image::open(&path)
        .with_context(|| format!("No se pudo cargar la imagen de fondo {}", path.display()))?
        .into_rgba8();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        decoded.as_raw(),
        decoded.width(),
        decoded.height(),
    );
    Ok(Some(Image::from_rgba8(buffer)))
}

fn background_image_opacity(appearance: &TerminalAppearance, focused: bool) -> f32 {
    let window_opacity = if focused {
        appearance.focused_opacity
    } else {
        appearance.unfocused_opacity
    };
    (f32::from(appearance.background_image_opacity) / 100.0)
        * (f32::from(window_opacity) / 100.0)
}

struct Glyph {
    metrics: Metrics,
    alpha: Vec<u8>,
}

struct TerminalModel {
    session: EmbeddedSession,
    parser: vt100::Parser,
    font: Font,
    glyphs: HashMap<(char, u16), Glyph>,
    selection: Option<(usize, usize)>,
    dragging: bool,
    cursor_on: bool,
    blink: Instant,
    dirty: bool,
    focused: bool,
    appearance: TerminalAppearance,
    width: u32,
    height: u32,
    scale: f32,
}

impl TerminalModel {
    fn new(appearance: TerminalAppearance) -> Result<Self> {
        let font = Font::from_bytes(NERD_FONT_BYTES, FontSettings::default())
            .map_err(|error| anyhow::anyhow!("No se pudo cargar la Nerd Font embebida: {error}"))?;

        Ok(Self {
            session: EmbeddedSession::start(INITIAL_COLS, INITIAL_ROWS)?,
            parser: vt100::Parser::new(INITIAL_ROWS, INITIAL_COLS, 10_000),
            font,
            glyphs: HashMap::new(),
            selection: None,
            dragging: false,
            cursor_on: true,
            blink: Instant::now(),
            dirty: true,
            focused: true,
            appearance,
            width: 0,
            height: 0,
            scale: 1.0,
        })
    }

    fn set_appearance(&mut self, appearance: TerminalAppearance) {
        self.appearance = appearance;
        // Force a geometry recalculation because content_top_gap can change.
        self.width = 0;
        self.height = 0;
        self.glyphs.clear();
        self.dirty = true;
    }

    fn input(&mut self, bytes: &[u8]) {
        self.parser.screen_mut().set_scrollback(0);
        self.selection = None;
        self.cursor_on = true;
        self.blink = Instant::now();
        self.dirty = true;
        let _ = self.session.write(bytes);
    }

    fn key_event(&mut self, code: CtKeyCode, modifiers: CtKeyModifiers) {
        self.parser.screen_mut().set_scrollback(0);
        self.selection = None;
        self.cursor_on = true;
        self.blink = Instant::now();
        self.dirty = true;
        let _ = self.session.send_key_event(CtKeyEvent::new(code, modifiers));
    }

    fn paste(&mut self, text: &str) {
        self.parser.screen_mut().set_scrollback(0);
        self.selection = None;
        self.cursor_on = true;
        self.blink = Instant::now();
        self.dirty = true;
        let _ = self.session.paste(text);
    }

    fn has_selection(&self) -> bool {
        self.selection.is_some_and(|(a, b)| a != b)
    }

    fn selected_text(&self) -> String {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let Some((a, b)) = self.selection else {
            return String::new();
        };

        let (start, end) = (a.min(b), a.max(b));
        let mut lines = Vec::new();
        for row in 0..rows {
            let mut line = String::new();
            let mut selected = false;
            for col in 0..cols {
                let index = row as usize * cols as usize + col as usize;
                if index < start || index > end {
                    continue;
                }
                selected = true;
                if let Some(cell) = screen.cell(row, col) {
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let text = cell.contents();
                    if text.is_empty() {
                        line.push(' ');
                    } else {
                        line.push_str(text);
                    }
                }
            }
            if selected {
                lines.push(line.trim_end().to_owned());
            }
        }
        lines.join("\r\n")
    }

    fn geometry(&self) -> (f32, f32, f32, f32) {
        let scale = self.scale.max(0.5);
        let left_pad = (PAD * scale).round();
        let top_pad = ((ISLAND_TOP
            + ISLAND_HEIGHT
            + self.appearance.content_top_gap as f32)
            * scale)
            .round()
            .max(left_pad);

        (
            left_pad,
            top_pad,
            (CELL_WIDTH * scale).round().max(1.0),
            (CELL_HEIGHT * scale).round().max(1.0),
        )
    }

    fn resize(&mut self, width: u32, height: u32, scale: f32) {
        let scale = scale.max(0.5);
        if self.width == width
            && self.height == height
            && (self.scale - scale).abs() < f32::EPSILON
        {
            return;
        }

        self.width = width.max(1);
        self.height = height.max(1);
        self.scale = scale;
        let (left_pad, top_pad, cell_width, cell_height) = self.geometry();
        let cols = (((self.width as f32 - left_pad * 2.0) / cell_width).floor() as i32)
            .clamp(2, 500) as u16;
        let rows = (((self.height as f32 - top_pad - left_pad) / cell_height).floor() as i32)
            .clamp(2, 200) as u16;

        if self.parser.screen().size() != (rows, cols) {
            self.parser.screen_mut().set_size(rows, cols);
            self.session.resize(cols, rows);
            self.selection = None;
        }
        self.glyphs.clear();
        self.dirty = true;
    }

    fn set_focused(&mut self, focused: bool) {
        if self.focused != focused {
            self.focused = focused;
            self.dirty = true;
        }
    }

    fn current_background_opacity(&self) -> u8 {
        if self.focused {
            self.appearance.focused_opacity
        } else {
            self.appearance.unfocused_opacity
        }
    }

    fn tick(&mut self) {
        while let Ok(data) = self.session.output.try_recv() {
            self.parser.process(&data);
            self.dirty = true;
        }

        if self.blink.elapsed() >= Duration::from_millis(550) {
            self.cursor_on = !self.cursor_on;
            self.blink = Instant::now();
            self.dirty = true;
        }

        self.session.check_timeout();
    }

    fn cell_at_logical(&self, x: f32, y: f32) -> usize {
        let physical_x = x * self.scale;
        let physical_y = y * self.scale;
        let (left_pad, top_pad, cell_width, cell_height) = self.geometry();
        let (rows, cols) = self.parser.screen().size();
        let col = (((physical_x - left_pad).max(0.0) / cell_width).floor() as i32)
            .clamp(0, cols as i32 - 1);
        let row = (((physical_y - top_pad).max(0.0) / cell_height).floor() as i32)
            .clamp(0, rows as i32 - 1);
        (row * cols as i32 + col) as usize
    }

    fn pointer_down(&mut self, x: f32, y: f32) {
        let index = self.cell_at_logical(x, y);
        self.selection = Some((index, index));
        self.dragging = true;
        self.dirty = true;
    }

    fn pointer_move(&mut self, x: f32, y: f32) {
        if !self.dragging {
            return;
        }
        let index = self.cell_at_logical(x, y);
        if let Some((start, _)) = self.selection {
            self.selection = Some((start, index));
            self.dirty = true;
        }
    }

    fn pointer_up(&mut self) {
        self.dragging = false;
    }

    fn scroll(&mut self, delta_y: f32) {
        if delta_y.abs() < 0.5 {
            return;
        }
        let rows = (delta_y / 40.0).round() as i32;
        let current = self.parser.screen().scrollback() as i32;
        self.parser
            .screen_mut()
            .set_scrollback((current + rows).max(0) as usize);
        self.selection = None;
        self.dirty = true;
    }

    fn render(&mut self) -> Image {
        let width = self.width.max(1);
        let height = self.height.max(1);
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        {
            let pixels = buffer.make_mut_slice();
            let background_opacity = self.current_background_opacity();
            if background_opacity > 0 {
                let bg = parse_rgb(&self.appearance.background_color).unwrap_or(BG);
                let alpha = ((u16::from(background_opacity) * 255) / 100) as u8;
                pixels.fill(Rgba8Pixel {
                    r: ((u16::from(bg.0) * u16::from(alpha)) / 255) as u8,
                    g: ((u16::from(bg.1) * u16::from(alpha)) / 255) as u8,
                    b: ((u16::from(bg.2) * u16::from(alpha)) / 255) as u8,
                    a: alpha,
                });
            } else {
                pixels.fill(Rgba8Pixel {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                });
            }

            let screen = self.parser.screen();
            let (rows, cols) = screen.size();
            let cursor = screen.cursor_position();
            let selection = self.selection;
            let cursor_on =
                self.cursor_on && screen.scrollback() == 0 && !screen.hide_cursor();
            let (left_pad, top_pad, cell_width, cell_height) = self.geometry();
            let font_px = (FONT_SIZE * self.scale).round().max(8.0);
            let font_key = font_px.round() as u16;

            for row in 0..rows {
                for col in 0..cols {
                    let Some(cell) = screen.cell(row, col) else {
                        continue;
                    };
                    if cell.is_wide_continuation() {
                        continue;
                    }

                    let mut fg = terminal_color(cell.fgcolor(), FG);
                    let mut bg = terminal_color(cell.bgcolor(), BG);
                    let mut paint_background =
                        !matches!(cell.bgcolor(), vt100::Color::Default) || cell.inverse();

                    if cell.inverse() {
                        std::mem::swap(&mut fg, &mut bg);
                    }

                    let index = row as usize * cols as usize + col as usize;
                    if selection
                        .is_some_and(|(a, b)| index >= a.min(b) && index <= a.max(b))
                    {
                        fg = Rgb(255, 255, 255);
                        bg = SELECTION;
                        paint_background = true;
                    }

                    if cursor_on && cursor == (row, col) {
                        fg = BG;
                        bg = CURSOR;
                        paint_background = true;
                    }

                    let x = (left_pad + col as f32 * cell_width).round() as i32;
                    let y = (top_pad + row as f32 * cell_height).round() as i32;
                    let wide = if cell.is_wide() { 2.0 } else { 1.0 };
                    let w = (cell_width * wide).ceil() as i32;
                    let h = cell_height.ceil() as i32;

                    if paint_background {
                        fill_rect(pixels, width, height, x, y, w, h, bg);
                    }

                    let content = cell.contents();
                    if content.is_empty() {
                        continue;
                    }

                    let mut pen_x = x;
                    let baseline = y + (cell_height * 0.80).round() as i32;
                    for ch in content.chars() {
                        let key = (ch, font_key);
                        if !self.glyphs.contains_key(&key) {
                            let (metrics, alpha) = self.font.rasterize(ch, font_px);
                            self.glyphs.insert(key, Glyph { metrics, alpha });
                        }
                        if let Some(glyph) = self.glyphs.get(&key) {
                            draw_glyph(
                                pixels,
                                width,
                                height,
                                pen_x,
                                baseline,
                                glyph,
                                fg,
                            );
                            if cell.bold() {
                                draw_glyph(
                                    pixels,
                                    width,
                                    height,
                                    pen_x + self.scale.max(1.0).round() as i32,
                                    baseline,
                                    glyph,
                                    fg,
                                );
                            }
                            pen_x += glyph.metrics.advance_width.round() as i32;
                        }
                    }
                }
            }
        }

        self.dirty = false;
        Image::from_rgba8_premultiplied(buffer)
    }
}

fn terminal_color(value: vt100::Color, default: Rgb) -> Rgb {
    const COLORS: [Rgb; 16] = [
        Rgb(0x11, 0x16, 0x29),
        Rgb(0xF2, 0x6B, 0x6B),
        Rgb(0xA3, 0xC7, 0x86),
        Rgb(0xE8, 0xCC, 0x83),
        Rgb(0x82, 0xAD, 0xE0),
        Rgb(0xC9, 0x9F, 0xCE),
        Rgb(0xC0, 0xCD, 0xD7),
        Rgb(0xDF, 0xE8, 0xEF),
        Rgb(0x67, 0x6E, 0x75),
        Rgb(0xFF, 0x87, 0x87),
        Rgb(0xC4, 0xEB, 0xA8),
        Rgb(0xFF, 0xE8, 0xA6),
        Rgb(0xA8, 0xD1, 0xFF),
        Rgb(0xEB, 0xC1, 0xF0),
        Rgb(0xE2, 0xEF, 0xF9),
        Rgb(0xFF, 0xFF, 0xFF),
    ];

    match value {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Rgb(r, g, b),
        vt100::Color::Idx(i) if i < 16 => COLORS[i as usize],
        vt100::Color::Idx(i) if i >= 232 => {
            let v = 8 + (i - 232) * 10;
            Rgb(v, v, v)
        }
        vt100::Color::Idx(i) => {
            let i = i - 16;
            let component = |n| if n == 0 { 0 } else { 55 + n * 40 };
            Rgb(component(i / 36), component(i / 6 % 6), component(i % 6))
        }
    }
}

fn fill_rect(
    pixels: &mut [Rgba8Pixel],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: Rgb,
) {
    let left = x.max(0) as u32;
    let top = y.max(0) as u32;
    let right = (x + w).max(0).min(width as i32) as u32;
    let bottom = (y + h).max(0).min(height as i32) as u32;

    for py in top..bottom {
        let start = (py * width + left) as usize;
        let end = (py * width + right) as usize;
        for pixel in &mut pixels[start..end] {
            *pixel = Rgba8Pixel {
                r: color.0,
                g: color.1,
                b: color.2,
                a: 255,
            };
        }
    }
}

fn draw_glyph(
    pixels: &mut [Rgba8Pixel],
    width: u32,
    height: u32,
    cell_x: i32,
    baseline: i32,
    glyph: &Glyph,
    color: Rgb,
) {
    if glyph.metrics.width == 0 || glyph.metrics.height == 0 {
        return;
    }

    let start_x = cell_x + glyph.metrics.xmin;
    let start_y =
        baseline - glyph.metrics.ymin - glyph.metrics.height as i32;

    for gy in 0..glyph.metrics.height {
        for gx in 0..glyph.metrics.width {
            let x = start_x + gx as i32;
            let y = start_y + gy as i32;
            if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                continue;
            }

            let alpha = glyph.alpha[gy * glyph.metrics.width + gx] as u16;
            if alpha == 0 {
                continue;
            }

            let index = y as usize * width as usize + x as usize;
            let dst = pixels[index];
            let inv = 255u16 - alpha;

            let sr = color.0 as u16 * alpha / 255;
            let sg = color.1 as u16 * alpha / 255;
            let sb = color.2 as u16 * alpha / 255;

            pixels[index] = Rgba8Pixel {
                r: (sr + dst.r as u16 * inv / 255).min(255) as u8,
                g: (sg + dst.g as u16 * inv / 255).min(255) as u8,
                b: (sb + dst.b as u16 * inv / 255).min(255) as u8,
                a: (alpha + dst.a as u16 * inv / 255).min(255) as u8,
            };
        }
    }
}

fn key_is(text: &str, key: slint::platform::Key) -> bool {
    let encoded: SharedString = key.into();
    text == encoded.as_str()
}

fn raw_key_code(text: &str, shift: bool) -> Option<CtKeyCode> {
    use slint::platform::Key;

    let special = [
        (Key::Escape, CtKeyCode::Esc),
        (Key::Return, CtKeyCode::Enter),
        (Key::Backspace, CtKeyCode::Backspace),
        (Key::UpArrow, CtKeyCode::Up),
        (Key::DownArrow, CtKeyCode::Down),
        (Key::LeftArrow, CtKeyCode::Left),
        (Key::RightArrow, CtKeyCode::Right),
        (Key::Home, CtKeyCode::Home),
        (Key::End, CtKeyCode::End),
        (Key::Delete, CtKeyCode::Delete),
        (Key::Insert, CtKeyCode::Insert),
        (Key::PageUp, CtKeyCode::PageUp),
        (Key::PageDown, CtKeyCode::PageDown),
        (Key::F1, CtKeyCode::F(1)),
        (Key::F2, CtKeyCode::F(2)),
        (Key::F3, CtKeyCode::F(3)),
        (Key::F4, CtKeyCode::F(4)),
        (Key::F5, CtKeyCode::F(5)),
        (Key::F6, CtKeyCode::F(6)),
        (Key::F7, CtKeyCode::F(7)),
        (Key::F8, CtKeyCode::F(8)),
        (Key::F9, CtKeyCode::F(9)),
        (Key::F10, CtKeyCode::F(10)),
        (Key::F11, CtKeyCode::F(11)),
        (Key::F12, CtKeyCode::F(12)),
    ];

    if key_is(text, Key::Tab) {
        return Some(if shift {
            CtKeyCode::BackTab
        } else {
            CtKeyCode::Tab
        });
    }
    if key_is(text, Key::Backtab) {
        return Some(CtKeyCode::BackTab);
    }
    for (key, code) in special {
        if key_is(text, key) {
            return Some(code);
        }
    }

    let mut chars = text.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && !ch.is_control()).then_some(CtKeyCode::Char(ch))
}

fn handle_key(
    model: &mut TerminalModel,
    text: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
) {
    use slint::platform::Key;

    if ctrl && text.eq_ignore_ascii_case("q") {
        model.session.force_abort();
        model.dirty = true;
        return;
    }

    if (ctrl && text.eq_ignore_ascii_case("c")) && model.has_selection() {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(model.selected_text());
        }
        return;
    }

    if ctrl && shift && text.eq_ignore_ascii_case("c") {
        let selected = model.selected_text();
        if !selected.is_empty() {
            if let Ok(mut clipboard) = Clipboard::new() {
                let _ = clipboard.set_text(selected);
            }
        }
        return;
    }

    if ctrl && key_is(text, Key::Insert) && model.has_selection() {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(model.selected_text());
        }
        return;
    }

    if (ctrl && text.eq_ignore_ascii_case("v")) || (shift && key_is(text, Key::Insert)) {
        if let Ok(mut clipboard) = Clipboard::new()
            && let Ok(value) = clipboard.get_text()
        {
            model.paste(&value);
        }
        return;
    }

    if !model.session.raw_mode() {
        let mut modifiers = CtKeyModifiers::NONE;
        if ctrl { modifiers |= CtKeyModifiers::CONTROL; }
        if alt { modifiers |= CtKeyModifiers::ALT; }
        if shift { modifiers |= CtKeyModifiers::SHIFT; }

        if ctrl && key_is(text, Key::Backspace) {
            model.key_event(CtKeyCode::Backspace, modifiers);
            return;
        }

        let navigation = if key_is(text, Key::LeftArrow) {
            Some(CtKeyCode::Left)
        } else if key_is(text, Key::RightArrow) {
            Some(CtKeyCode::Right)
        } else if key_is(text, Key::Home) {
            Some(CtKeyCode::Home)
        } else if key_is(text, Key::End) {
            Some(CtKeyCode::End)
        } else {
            None
        };

        if navigation.is_some() && (shift || ctrl) {
            model.key_event(navigation.unwrap(), modifiers);
            return;
        }
    }

    if model.session.raw_mode() {
        let altgr_like = ctrl && alt && text.chars().count() == 1;
        let mut modifiers = CtKeyModifiers::NONE;
        if alt && !altgr_like {
            modifiers |= CtKeyModifiers::ALT;
        }
        if ctrl && !altgr_like {
            modifiers |= CtKeyModifiers::CONTROL;
        }
        if shift {
            modifiers |= CtKeyModifiers::SHIFT;
        }

        if let Some(code) = raw_key_code(text, shift) {
            let _ = model.session.send_raw_key(CtKeyEvent::new(code, modifiers));
            model.parser.screen_mut().set_scrollback(0);
            model.selection = None;
            model.dirty = true;
        }
        return;
    }

    if key_is(text, Key::Return) {
        model.input(b"\r");
    } else if key_is(text, Key::Backspace) {
        model.input(b"\x7f");
    } else if key_is(text, Key::Tab) {
        model.input(if shift { b"\x1b[Z" } else { b"\t" });
    } else if key_is(text, Key::Backtab) {
        model.input(b"\x1b[Z");
    } else if key_is(text, Key::Escape) {
        model.input(b"\x1b");
    } else if key_is(text, Key::UpArrow) {
        let sequence = if model.parser.screen().application_cursor() {
            b"\x1bOA".as_slice()
        } else {
            b"\x1b[A".as_slice()
        };
        model.input(sequence);
    } else if key_is(text, Key::DownArrow) {
        let sequence = if model.parser.screen().application_cursor() {
            b"\x1bOB".as_slice()
        } else {
            b"\x1b[B".as_slice()
        };
        model.input(sequence);
    } else if key_is(text, Key::RightArrow) {
        model.input(b"\x1b[C");
    } else if key_is(text, Key::LeftArrow) {
        model.input(b"\x1b[D");
    } else if key_is(text, Key::Home) {
        model.input(b"\x1b[H");
    } else if key_is(text, Key::End) {
        model.input(b"\x1b[F");
    } else if key_is(text, Key::Delete) {
        model.input(b"\x1b[3~");
    } else if key_is(text, Key::Insert) {
        model.input(b"\x1b[2~");
    } else if key_is(text, Key::PageUp) {
        model.input(b"\x1b[5~");
    } else if key_is(text, Key::PageDown) {
        model.input(b"\x1b[6~");
    } else {
        let altgr_like = ctrl && alt;
        if ctrl && !altgr_like {
            if text == " " || key_is(text, Key::Space) {
                model.input(&[0]);
                return;
            }
            if let Some(ch) = text.chars().next()
                && text.chars().count() == 1
                && ch.is_ascii_alphabetic()
            {
                model.input(&[(ch.to_ascii_uppercase() as u8) & 0x1f]);
                return;
            }
        }

        if alt && !altgr_like {
            model.input(b"\x1b");
        }
        if !text.is_empty() {
            model.input(text.as_bytes());
        }
    }
}

#[repr(C)]
struct AccentPolicy {
    accent_state: i32,
    accent_flags: i32,
    gradient_color: u32,
    animation_id: i32,
}

#[repr(C)]
struct WindowCompositionAttributeData {
    attribute: i32,
    data: *mut c_void,
    size_of_data: usize,
}

const WCA_ACCENT_POLICY: i32 = 19;
const ACCENT_DISABLED: i32 = 0;
const ACCENT_ENABLE_BLUR_BEHIND: i32 = 3;
const ACCENT_ENABLE_ACRYLIC_BLUR_BEHIND: i32 = 4;

unsafe fn apply_window_effects(hwnd: HWND, appearance: &TerminalAppearance) {
    unsafe {
        let dark: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            20,
            (&dark as *const i32).cast(),
            size_of::<i32>() as u32,
        );

        // SST paints/composes its own full-window surface.
        let backdrop_none: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            38,
            (&backdrop_none as *const i32).cast(),
            size_of::<i32>() as u32,
        );

        let corners: i32 = 2;
        let _ = DwmSetWindowAttribute(
            hwnd,
            33,
            (&corners as *const i32).cast(),
            size_of::<i32>() as u32,
        );

        apply_configured_backdrop(hwnd, appearance, true);

        if appearance.backdrop != "solid" {
            // Glass covers the complete client area. There is no titlebar strip;
            // the Slint island is the only opaque control surface at the top.
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        }
    }
}

unsafe fn apply_configured_backdrop(
    hwnd: HWND,
    appearance: &TerminalAppearance,
    focused: bool,
) {
    unsafe {
        let user32_name: Vec<u16> = "user32.dll".encode_utf16().chain(Some(0)).collect();
        let user32 = GetModuleHandleW(user32_name.as_ptr());
        if user32.is_null() {
            return;
        }

        let Some(proc) = GetProcAddress(user32, b"SetWindowCompositionAttribute\0".as_ptr()) else {
            return;
        };

        type SetWindowCompositionAttributeFn =
            unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> i32;
        let set_attribute: SetWindowCompositionAttributeFn = std::mem::transmute(proc);

        let opacity = if focused {
            appearance.focused_opacity
        } else {
            appearance.unfocused_opacity
        };

        let state = if opacity == 0 {
            ACCENT_DISABLED
        } else {
            match appearance.backdrop.as_str() {
                "acrylic" => ACCENT_ENABLE_ACRYLIC_BLUR_BEHIND,
                "blur" | "glass" => ACCENT_ENABLE_BLUR_BEHIND,
                _ => ACCENT_DISABLED,
            }
        };

        let Rgb(r, g, b) = parse_rgb(&appearance.background_color).unwrap_or(BG);
        let tint_bgr = r as u32 | ((g as u32) << 8) | ((b as u32) << 16);
        let alpha = ((u32::from(opacity) * 255) / 100) << 24;

        let mut policy = AccentPolicy {
            accent_state: state,
            accent_flags: 2,
            gradient_color: alpha | tint_bgr,
            animation_id: 0,
        };
        let mut data = WindowCompositionAttributeData {
            attribute: WCA_ACCENT_POLICY,
            data: (&mut policy as *mut AccentPolicy).cast(),
            size_of_data: size_of::<AccentPolicy>(),
        };
        let _ = set_attribute(hwnd, &mut data);
    }
}

fn slint_hwnd(ui: &SstBlueWindow) -> Option<HWND> {
    let handle = ui.window().window_handle();
    let window_handle = handle.window_handle().ok()?;
    let RawWindowHandle::Win32(win32) = window_handle.as_raw() else {
        return None;
    };
    Some(win32.hwnd.get() as HWND)
}

unsafe fn apply_native_window_region(
    hwnd: HWND,
    width: u32,
    height: u32,
    corner_radius: u16,
) {
    unsafe {
        if IsZoomed(hwnd) != 0 || corner_radius == 0 {
            SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
            return;
        }

        let radius = i32::from(corner_radius);
        let diameter = (radius * 2).max(1);
        let region = CreateRoundRectRgn(
            0,
            0,
            width.min(i32::MAX as u32) as i32 + 1,
            height.min(i32::MAX as u32) as i32 + 1,
            diameter,
            diameter,
        );

        if !region.is_null() && SetWindowRgn(hwnd, region, 1) == 0 {
            DeleteObject(region);
        }
    }
}

fn apply_slint_window_effects(ui: &SstBlueWindow, appearance: &TerminalAppearance) {
    if let Some(hwnd) = slint_hwnd(ui) {
        unsafe { apply_window_effects(hwnd, appearance) };
    }
}

pub fn run() -> Result<()> {
    // The terminal surface needs a genuinely transparent native window.
    // Setting only `background: transparent` in Slint is not enough on every
    // Windows/FemtoVG combination because the native Winit window may otherwise
    // be created as opaque before the renderer starts.
    BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("femtovg".into())
        .with_winit_window_attributes_hook(|attributes| {
            attributes
                .with_transparent(true)
                .with_decorations(false)
        })
        .select()
        .map_err(|error| anyhow::anyhow!("No se pudo inicializar Winit/FemtoVG transparente: {error}"))?;

    let paths = AppPaths::detect();
    paths.ensure_layout()?;
    let appearance = paths.load_appearance()?;
    let background_image = load_background_image(&paths, &appearance)?;
    let appearance_state = Rc::new(RefCell::new(appearance.clone()));
    let model = Rc::new(RefCell::new(TerminalModel::new(appearance.clone())?));
    let metrics = Rc::new(RefCell::new(System::new_all()));
    let ui = SstBlueWindow::new()?;

    if let Some(image) = background_image {
        ui.set_background_image(image);
    }
    ui.set_background_image_opacity(background_image_opacity(&appearance, true));
    ui.set_window_corner_radius((appearance.corner_radius as f32).into());

    {
        let model = model.clone();
        ui.on_key_input(move |text, ctrl, alt, shift| {
            handle_key(&mut model.borrow_mut(), text.as_str(), ctrl, alt, shift);
        });
    }
    {
        let model = model.clone();
        ui.on_pointer_down(move |x, y| {
            model.borrow_mut().pointer_down(x, y);
        });
    }
    {
        let model = model.clone();
        ui.on_pointer_move(move |x, y| {
            model.borrow_mut().pointer_move(x, y);
        });
    }
    {
        let model = model.clone();
        ui.on_pointer_up(move |_x, _y, right| {
            let mut model = model.borrow_mut();
            if right {
                if let Ok(mut clipboard) = Clipboard::new()
                    && let Ok(value) = clipboard.get_text()
                {
                    model.paste(&value);
                }
            } else {
                model.pointer_up();
            }
        });
    }
    {
        let model = model.clone();
        ui.on_pointer_scroll(move |delta| {
            model.borrow_mut().scroll(delta);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_window(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.hide();
            }
            let _ = slint::quit_event_loop();
        });
    }

    ui.show()?;

    // The Win32 handle exists only after the winit window has been created.
    {
        let weak = ui.as_weak();
        let appearance = appearance.clone();
        Timer::single_shot(Duration::ZERO, move || {
            if let Some(ui) = weak.upgrade() {
                apply_slint_window_effects(&ui, &appearance);
                if let Some(hwnd) = slint_hwnd(&ui) {
                    let size = ui.window().size();
                    unsafe {
                        apply_native_window_region(
                            hwnd,
                            size.width,
                            size.height,
                            appearance.corner_radius,
                        );
                    }
                }
            }
        });
    }

    let weak = ui.as_weak();
    let last_status = Rc::new(RefCell::new(Instant::now() - Duration::from_secs(2)));
    let last_focus = Rc::new(RefCell::new(true));
    let last_region = Rc::new(RefCell::new((0u32, 0u32, false)));
    let timer = Timer::default();
    {
        let model = model.clone();
        let metrics = metrics.clone();
        let last_status = last_status.clone();
        let last_focus = last_focus.clone();
        let last_region = last_region.clone();
        let appearance_state = appearance_state.clone();
        let paths = paths.clone();

        timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };

            let size = ui.window().size();
            let scale = ui.window().scale_factor();

            let reload_requested = {
                let model = model.borrow();
                model.session.take_config_reload_request()
            };

            if reload_requested {
                match paths.load_appearance()
                    .and_then(|next| {
                        let image = load_background_image(&paths, &next)?;
                        Ok((next, image))
                    })
                {
                    Ok((next, image)) => {
                        let focused = slint_hwnd(&ui)
                            .is_some_and(|hwnd| unsafe { GetForegroundWindow() == hwnd });

                        ui.set_background_image(image.unwrap_or_default());
                        ui.set_background_image_opacity(
                            background_image_opacity(&next, focused)
                        );
                        ui.set_window_corner_radius((next.corner_radius as f32).into());

                        model.borrow_mut().set_appearance(next.clone());
                        *appearance_state.borrow_mut() = next.clone();

                        if let Some(hwnd) = slint_hwnd(&ui) {
                            unsafe {
                                apply_configured_backdrop(hwnd, &next, focused);
                                apply_native_window_region(
                                    hwnd,
                                    size.width,
                                    size.height,
                                    next.corner_radius,
                                );
                            }
                        }

                        *last_region.borrow_mut() = (0, 0, false);
                    }
                    Err(error) => {
                        eprintln!("No se pudo recargar la apariencia de SST: {error}");
                    }
                }
            }

            if let Some(hwnd) = slint_hwnd(&ui) {
                let focused = unsafe { GetForegroundWindow() == hwnd };
                let maximized = unsafe { IsZoomed(hwnd) != 0 };
                let appearance = appearance_state.borrow().clone();

                let mut previous = last_focus.borrow_mut();
                if *previous != focused {
                    *previous = focused;
                    model.borrow_mut().set_focused(focused);
                    ui.set_background_image_opacity(background_image_opacity(&appearance, focused));
                    unsafe { apply_configured_backdrop(hwnd, &appearance, focused) };
                }

                let mut region_state = last_region.borrow_mut();
                let current_region = (size.width, size.height, maximized);
                if *region_state != current_region {
                    *region_state = current_region;
                    unsafe {
                        apply_native_window_region(
                            hwnd,
                            size.width,
                            size.height,
                            appearance.corner_radius,
                        );
                    }
                }
            }

            {
                let mut model = model.borrow_mut();
                model.resize(size.width, size.height, scale);
                model.tick();

                if model.dirty {
                    ui.set_terminal_image(model.render());
                }

                if model.session.exited() {
                    let _ = ui.hide();
                    let _ = slint::quit_event_loop();
                    return;
                }
            }

            if last_status.borrow().elapsed() >= Duration::from_secs(1) {
                let mut system = metrics.borrow_mut();
                system.refresh_all();

                let cpu = if system.cpus().is_empty() {
                    0.0
                } else {
                    system.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>()
                        / system.cpus().len() as f32
                };
                let ram_gib = system.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
                let now = Local::now();

                ui.set_cpu_text(format!("CPU {:.0}%", cpu).into());
                ui.set_ram_text(format!("RAM {:.1} GiB", ram_gib).into());
                ui.set_clock_text(now.format("%H:%M").to_string().into());
                ui.set_date_text(now.format("%d %b").to_string().into());
                *last_status.borrow_mut() = Instant::now();
            }

        });
    }

    slint::run_event_loop()?;
    timer.stop();
    let _ = ui.hide();
    Ok(())
}
