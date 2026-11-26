//! Slint/FemtoVG frontend for SST Blueversion.
//!
//! The terminal owns the whole client area. The top status island is an overlay:
//! there is deliberately no full-width title bar behind it.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_void,
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
    Graphics::Dwm::*,
    System::{
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Threading::{AttachThreadInput, GetCurrentThreadId},
    },
    UI::{
        Controls::MARGINS,
        Input::KeyboardAndMouse::{SetActiveWindow, SetFocus},
        WindowsAndMessaging::{
            BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsZoomed,
            GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_STYLE,
            WS_CAPTION, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, SWP_NOACTIVATE,
            SetForegroundWindow, SetWindowTextW, ShowWindow, SW_RESTORE,
            LoadIconW, SendMessageW, WM_SETICON, WM_GETICON, ICON_BIG, ICON_SMALL,
            SetClassLongPtrW, GCLP_HICON, GCLP_HICONSM,
        },
    },
};

use crate::adapters::{
    persistence::{AppPaths, AppearanceConfig},
    terminal::{embedded::EmbeddedSession,io::DisplayPacket},
};

const INITIAL_COLS: u16 = 112;
const INITIAL_ROWS: u16 = 31;
const ISLAND_TOP: f32 = 6.0;
const ISLAND_HEIGHT: f32 = 34.0;

// The frameless Slint window can briefly report an icon-sized client area
// *before* IsIconic becomes true, or *after* it becomes false. Never send
// those transient dimensions to the PTY: vt100 reflows its screen irreversibly.
fn valid_terminal_viewport(width: u32, height: u32, scale: f32, minimized: bool) -> bool {
    if minimized || !scale.is_finite() {
        return false;
    }
    let scale = scale.max(0.5);
    width as f32 >= 360.0 * scale && height as f32 >= 220.0 * scale
}

#[derive(Default)]
struct RestoreViewportGuard {
    // A minimized/transient viewport must be followed by two consecutive
    // identical, valid frame sizes before resizing the PTY again.
    restoring: bool,
    candidate: Option<(u32, u32)>,
    stable_frames: u8,
}

impl RestoreViewportGuard {
    fn drawable(&mut self, width: u32, height: u32, scale: f32, minimized: bool) -> bool {
        if !valid_terminal_viewport(width, height, scale, minimized) {
            self.restoring = true;
            self.candidate = None;
            self.stable_frames = 0;
            return false;
        }
        if !self.restoring {
            return true;
        }
        if self.candidate == Some((width, height)) {
            self.stable_frames = self.stable_frames.saturating_add(1);
        } else {
            self.candidate = Some((width, height));
            self.stable_frames = 1;
        }
        if self.stable_frames < 2 {
            return false;
        }
        self.restoring = false;
        self.candidate = None;
        self.stable_frames = 0;
        true
    }
}

#[cfg(test)]
mod viewport_regression_tests {
    use super::{RestoreViewportGuard, valid_terminal_viewport};

    #[test]
    fn legacy_cyan_and_blue_accents_are_neutralized_globally() {
        assert_eq!(super::gruvbox_terminal_color(super::Rgb(69, 209, 241)),
            super::Rgb(131, 165, 152));
        assert_eq!(super::gruvbox_terminal_color(super::Rgb(80, 150, 255)),
            super::Rgb(235, 219, 178));
        assert_eq!(super::gruvbox_terminal_color(super::Rgb(69, 133, 136)),
            super::Rgb(131, 165, 152));
        assert_eq!(super::gruvbox_terminal_color(super::Rgb(184, 187, 38)),
            super::Rgb(184, 187, 38));
        assert_eq!(super::gruvbox_terminal_color(super::Rgb(251, 73, 52)),
            super::Rgb(251, 73, 52));
    }

    #[test]
    fn original_sst_icon_rgb_is_preserved_in_the_terminal() {
        for rgb in [
            super::Rgb(35, 52, 106),
            super::Rgb(54, 76, 119),
            super::Rgb(109, 143, 171),
            super::Rgb(186, 189, 198),
        ] {
            assert_eq!(super::gruvbox_terminal_color(rgb), rgb);
            assert_eq!(
                super::terminal_color(vt100::Color::Rgb(rgb.0, rgb.1, rgb.2), super::FG),
                rgb,
            );
        }
    }

    #[test]
    fn reject_iconic_and_transient_window_sizes() {
        assert!(!valid_terminal_viewport(0, 0, 1.0, false));
        assert!(!valid_terminal_viewport(150, 30, 1.0, false));
        assert!(!valid_terminal_viewport(980, 640, 1.0, true));
        assert!(!valid_terminal_viewport(450, 260, 1.5, false));
        assert!(valid_terminal_viewport(980, 640, 1.0, false));
        assert!(valid_terminal_viewport(980, 640, 2.0, false));
    }

    #[test]
    fn do_not_restore_pty_until_client_size_is_stable() {
        let mut guard = RestoreViewportGuard::default();
        assert!(guard.drawable(980, 640, 1.0, false));
        assert!(!guard.drawable(0, 0, 1.0, true));
        assert!(!guard.drawable(150, 30, 1.0, false));
        assert!(!guard.drawable(420, 300, 1.0, false));
        assert!(!guard.drawable(980, 640, 1.0, false));
        assert!(guard.drawable(980, 640, 1.0, false));
        assert!(guard.drawable(980, 640, 1.0, false));
    }
}

const FG: Rgb = Rgb(crate::presentation::formatter::DEFAULT_FOREGROUND.0,crate::presentation::formatter::DEFAULT_FOREGROUND.1,crate::presentation::formatter::DEFAULT_FOREGROUND.2);
const BG: Rgb = Rgb(0x28, 0x28, 0x28);
use crate::presentation::formatter::{caret_color, SELECTED_RGB};

const NERD_FONT_BYTES: &[u8] = crate::application::system::SST_TERMINAL_FONT_BYTES;

slint::slint! {
    // Rendered vector outlines, never made of VT100/ASCII characters.
    // Transparent interiors preserve the user's terminal artwork and logo.
    // Text-report section headings are individual native vector outlines,
    // derived from semantic RGB markers in the VT100 viewport. No fake lines.
    export global OutputTheme {
        in property <color> rail;
        in property <color> rule;
    }
    component ReportHeadingBand inherits Rectangle {
        // Restrained semantic outline: no cyan wash, no opaque overlay.
        background: transparent;
        border-width: 1px;
        border-color: OutputTheme.rule;
        border-radius: 6px;

        Rectangle {
            x: 1px;
            y: 3px;
            width: 3px;
            height: max(4px, parent.height - 6px);
            border-radius: 2px;
            background: OutputTheme.rail;
        }
    }

    component TerminalSectionFrame inherits Rectangle {
        background: transparent;
        border-width: 1px;
        border-color: OutputTheme.rule;
        border-radius: 9px;

        Rectangle {
            x: 1px;
            y: 7px;
            width: 3px;
            height: min(24px, parent.height - 14px);
            border-radius: 2px;
            background: OutputTheme.rail;
        }
    }

    // The download panel is independent of the VT100 pixel surface.
    // It appears only during a transfer and never paints over fetch or
    // modifies the user's background/transparency/terminal rows.
    // Inline transfer graphic. The command reserves terminal rows and prints
    // its own label. This component fills ONLY those rows: no floating window,
    // popup, opaque card, shadow, frame, or toolbar.
    component InlineTransferBar inherits Rectangle {
        in property <string> transfer-percent;
        in property <string> transfer-amount;
        in property <string> transfer-speed;
        in property <string> transfer-eta;
        in property <float> transfer-ratio: 0.0;
        in property <bool> transfer-known-total: false;

        background: transparent;
        border-width: 0px;

        track := Rectangle {
            x: 4px;
            y: 6px;
            width: parent.width - 144px;
            height: 10px;
            background: #3c3836;
            border-radius: 5px;
            clip: true;

            Rectangle {
                x: 0px;
                y: 0px;
                width: root.transfer-known-total ? parent.width * root.transfer-ratio : 0px;
                height: parent.height;
                background: #d79921;
                border-radius: 5px;

                Rectangle {
                    x: 1px;
                    y: 1px;
                    width: parent.width - 2px;
                    height: 3px;
                    background: #fabd2f;
                    border-radius: 2px;
                }
            }
        }

        Text {
            x: parent.width - 131px;
            y: 0px;
            width: 126px;
            height: 22px;
            text: root.transfer-percent;
            color: #fabd2f;
            font-family: "Segoe UI Variable";
            font-size: 16px;
            font-weight: 700;
            horizontal-alignment: right;
        }

        Text {
            x: 4px;
            y: 23px;
            width: parent.width - 9px;
            height: 19px;
            text: root.transfer-amount + "   ·   " + root.transfer-speed + "   ·   ETA " + root.transfer-eta;
            color: #d5c4a1;
            font-family: "Segoe UI Variable";
            font-size: 12px;
            overflow: elide;
        }
    }

    export component SstBlueWindow inherits Window {
        title: "Shell Shock Tool";
        preferred-width: 980px;
        preferred-height: 640px;
        min-width: 480px;
        min-height: 300px;
        no-frame: true;
        resize-border-width: 7px;
        background: transparent;

        in property <image> terminal-image;
        in property <bool> caret-visible;
        in property <float> caret-x;
        in property <float> caret-y;
        in property <float> caret-height;
        in property <color> caret-tone;
        in property <image> keygen-image;
        in property <bool> keygen-visible;
        in property <int> report-heading-count: 0;
        in property <float> report-grid-left: 0.0;
        in property <float> report-grid-top: 0.0;
        in property <float> report-grid-width: 0.0;
        in property <float> report-line-height: 18.0;
        in property <float> report-a-row: 0.0;
        in property <float> report-a-width: 0.0;
        in property <float> report-b-row: 0.0;
        in property <float> report-b-width: 0.0;
        in property <float> report-c-row: 0.0;
        in property <float> report-c-width: 0.0;
        in property <float> report-d-row: 0.0;
        in property <float> report-d-width: 0.0;
        in property <float> report-e-row: 0.0;
        in property <float> report-e-width: 0.0;
        in property <float> report-f-row: 0.0;
        in property <float> report-f-width: 0.0;

        in property <int> monitor-panel-count: 0;
        in property <float> monitor-grid-left: 0.0;
        in property <float> monitor-grid-top: 0.0;
        in property <float> monitor-grid-width: 0.0;
        in property <float> monitor-line-height: 18.0;
        in property <float> monitor-a-row: 0.0;
        in property <float> monitor-a-rows: 0.0;
        in property <float> monitor-b-row: 0.0;
        in property <float> monitor-b-rows: 0.0;
        in property <float> monitor-c-row: 0.0;
        in property <float> monitor-c-rows: 0.0;
        in property <float> monitor-d-row: 0.0;
        in property <float> monitor-d-rows: 0.0;
        in property <image> background-image;
        in property <float> background-image-opacity: 0.0;
        in property <string> background-image-fit-mode: "cover";
        in property <string> privilege-label: "TOKEN ?";
        in property <string> cpu-text: "CPU 0%";
        in property <string> ram-text: "RAM 0.0 GiB";
        in property <string> clock-text: "00:00";
        in property <string> date-text: "00 ---";
        // Logical coordinates align the live transfer indicator to the command output.
        in property <float> transfer-x: 22.0;
        in property <float> transfer-y: 65.0;
        in property <bool> transfer-visible: false;
        in property <bool> transfer-known-total: false;
        in property <float> transfer-ratio: 0.0;
        in property <string> transfer-percent: "";
        in property <string> transfer-amount: "";
        in property <string> transfer-speed: "";
        in property <string> transfer-eta: "";

        callback key-input(string, bool, bool, bool);
        callback pointer-down(float, float);
        callback pointer-move(float, float);
        callback pointer-up(float, float, bool);
        callback pointer-scroll(float);
        callback close-window();
        callback window-state-change();

        surface := Rectangle {
            x: 0;
            y: 0;
            width: 100%;
            height: 100%;
            background: transparent;

            Image {
                x: 0;
                y: 0;
                width: 100%;
                height: 100%;
                source: root.background-image;
                image-fit: root.background-image-fit-mode == "contain"
                    ? contain
                    : root.background-image-fit-mode == "fill"
                        ? fill
                        : root.background-image-fit-mode == "preserve"
                            ? preserve
                            : cover;
                opacity: root.background-image-opacity;
            }

            Image {
                x: 0;
                y: 0;
                width: 100%;
                height: 100%;
                visible: !root.keygen-visible;
                source: root.terminal-image;
                image-fit: fill;
            }

            if root.caret-visible && !root.keygen-visible: Rectangle {
                x: root.caret-x * 1px;
                y: root.caret-y * 1px;
                width: 2px;
                height: root.caret-height * 1px;
                background: root.caret-tone;
            }

            // Ordinary SST help and reports receive the same genuine native
            // section accents. No box characters enter the command stream.
            ReportHeadingBand {
                visible: !root.keygen-visible && root.report-heading-count > 0;
                x: root.report-grid-left * 1px;
                y: (root.report-grid-top + root.report-a-row * root.report-line-height) * 1px;
                width: root.report-a-width * 1px;
                height: root.report-line-height * 1px;
            }

            ReportHeadingBand {
                visible: !root.keygen-visible && root.report-heading-count > 1;
                x: root.report-grid-left * 1px;
                y: (root.report-grid-top + root.report-b-row * root.report-line-height) * 1px;
                width: root.report-b-width * 1px;
                height: root.report-line-height * 1px;
            }

            ReportHeadingBand {
                visible: !root.keygen-visible && root.report-heading-count > 2;
                x: root.report-grid-left * 1px;
                y: (root.report-grid-top + root.report-c-row * root.report-line-height) * 1px;
                width: root.report-c-width * 1px;
                height: root.report-line-height * 1px;
            }

            ReportHeadingBand {
                visible: !root.keygen-visible && root.report-heading-count > 3;
                x: root.report-grid-left * 1px;
                y: (root.report-grid-top + root.report-d-row * root.report-line-height) * 1px;
                width: root.report-d-width * 1px;
                height: root.report-line-height * 1px;
            }

            ReportHeadingBand {
                visible: !root.keygen-visible && root.report-heading-count > 4;
                x: root.report-grid-left * 1px;
                y: (root.report-grid-top + root.report-e-row * root.report-line-height) * 1px;
                width: root.report-e-width * 1px;
                height: root.report-line-height * 1px;
            }

            ReportHeadingBand {
                visible: !root.keygen-visible && root.report-heading-count > 5;
                x: root.report-grid-left * 1px;
                y: (root.report-grid-top + root.report-f-row * root.report-line-height) * 1px;
                width: root.report-f-width * 1px;
                height: root.report-line-height * 1px;
            }

            // Draw only for a TUI that explicitly supplies semantic sections.
            // The normal terminal (and its logo) has no vector overlays.
            TerminalSectionFrame {
                visible: !root.keygen-visible && root.monitor-panel-count > 0;
                x: root.monitor-grid-left * 1px;
                y: (root.monitor-grid-top + root.monitor-a-row * root.monitor-line-height) * 1px;
                width: root.monitor-grid-width * 1px;
                height: (root.monitor-a-rows * root.monitor-line-height) * 1px;
            }

            TerminalSectionFrame {
                visible: !root.keygen-visible && root.monitor-panel-count > 1;
                x: root.monitor-grid-left * 1px;
                y: (root.monitor-grid-top + root.monitor-b-row * root.monitor-line-height) * 1px;
                width: root.monitor-grid-width * 1px;
                height: (root.monitor-b-rows * root.monitor-line-height) * 1px;
            }

            TerminalSectionFrame {
                visible: !root.keygen-visible && root.monitor-panel-count > 2;
                x: root.monitor-grid-left * 1px;
                y: (root.monitor-grid-top + root.monitor-c-row * root.monitor-line-height) * 1px;
                width: root.monitor-grid-width * 1px;
                height: (root.monitor-c-rows * root.monitor-line-height) * 1px;
            }

            TerminalSectionFrame {
                visible: !root.keygen-visible && root.monitor-panel-count > 3;
                x: root.monitor-grid-left * 1px;
                y: (root.monitor-grid-top + root.monitor-d-row * root.monitor-line-height) * 1px;
                width: root.monitor-grid-width * 1px;
                height: (root.monitor-d-rows * root.monitor-line-height) * 1px;
            }

            if root.keygen-visible : Rectangle {
                width: 100%;
                height: 100%;
                background: transparent;
                Image {
                    width: 100%;
                    height: 100%;
                    source: root.keygen-image;
                    image-fit: contain;
                }
            }
        terminal-focus := FocusScope {
            x: 0;
            y: 0;
            width: 100%;
            height: 100%;
            focus-on-click: true;
            focus-on-tab-navigation: false;

            init => {
                self.focus();
            }

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
                x: 0;
                y: 0;
                width: 100%;
                height: 100%;
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

        // This graphic occupies lines reserved by the command *inside* SST.
        // It is not a floating modal/card and does not obstruct other output.
        InlineTransferBar {
            visible: root.transfer-visible && !root.keygen-visible;
            x: root.transfer-x * 1px;
            y: root.transfer-y * 1px;
            width: min(585px, root.width - root.transfer-x * 1px - 18px);
            height: 43px;
            transfer-percent: root.transfer-percent;
            transfer-amount: root.transfer-amount;
            transfer-speed: root.transfer-speed;
            transfer-eta: root.transfer-eta;
            transfer-ratio: root.transfer-ratio;
            transfer-known-total: root.transfer-known-total;
        }

        island := Rectangle {
            width: min(720px, root.width - 20px);
            height: 34px;
            x: (root.width - self.width) / 2;
            y: 6px;
            border-radius: 13px;
            background: rgba(40, 40, 40, 0.93);
            border-width: 1px;
            border-color: #504945;

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
                    color: #ebdbb2;
                    font-family: "Segoe UI Variable";
                    font-size: 15px;
                    font-weight: 700;
                    vertical-alignment: center;
                }

                // Always display the real process-token level, including a
                // normal/filtered session. Only elevated tokens use amber.
                Rectangle {
                    visible: true;
                    x: 70px;
                    y: 5px;
                    width: 96px;
                    height: 24px;
                    border-radius: 7px;
                    background: (root.privilege-label == "USUARIO" || root.privilege-label == "LIMITADO")
                        ? #3c3836 : #504030;
                    border-width: 1px;
                    border-color: (root.privilege-label == "USUARIO" || root.privilege-label == "LIMITADO")
                        ? #665c54 : #d79921;

                    Text {
                        x: 2px;
                        y: 0;
                        width: parent.width - 4px;
                        height: parent.height;
                        text: root.privilege-label;
                        color: (root.privilege-label == "USUARIO" || root.privilege-label == "LIMITADO")
                            ? #ebdbb2 : #fabd2f;
                        font-family: "Segoe UI Variable";
                        font-size: 11px;
                        font-weight: 700;
                        horizontal-alignment: center;
                        vertical-alignment: center;
                    }
                }

                Text {
                    visible: island.width >= 705px;
                    x: 184px;
                    y: 0;
                    width: 92px;
                    height: parent.height;
                    text: root.cpu-text;
                    color: #ebdbb2;
                    font-family: "Segoe UI Variable";
                    font-size: 13px;
                    vertical-alignment: center;
                }

                Text {
                    visible: island.width >= 705px;
                    x: 281px;
                    y: 0;
                    width: 128px;
                    height: parent.height;
                    text: root.ram-text;
                    color: #ebdbb2;
                    font-family: "Segoe UI Variable";
                    font-size: 13px;
                    vertical-alignment: center;
                }

                Text {
                    visible: island.width >= 525px;
                    x: island.width - 296px;
                    y: 0;
                    width: 62px;
                    height: parent.height;
                    text: root.clock-text;
                    color: #ebdbb2;
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
                    color: #bdae93;
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
                        ? rgb(80, 73, 69)
                        : minimize-touch.has-hover ? rgb(60, 56, 54) : transparent;

                    Path {
                        x: 11px;
                        y: 10px;
                        width: 16px;
                        height: 12px;
                        commands: root.maximized
                            ? "M 1 6 L 15 6"
                            : "M 1 1 L 8 9 L 15 1";
                        stroke: #d5c4a1;
                        stroke-width: 2.4px;
                        stroke-line-cap: round;
                        stroke-line-join: round;
                    }

                    minimize-touch := TouchArea {
                        mouse-cursor: pointer;
                        clicked => {
                            root.window-state-change();
                            root.minimized = true;
                        }
                    }
                }

                Rectangle {
                    x: island.width - 76px;
                    y: 1px;
                    width: 38px;
                    height: island.height - 2px;
                    border-radius: 10px;
                    background: maximize-touch.pressed
                        ? rgb(80, 73, 69)
                        : maximize-touch.has-hover ? rgb(60, 56, 54) : transparent;

                    Path {
                        x: 11px;
                        y: 11px;
                        width: 16px;
                        height: 12px;
                        commands: root.maximized
                            ? "M 1 1 L 8 9 L 15 1"
                            : "M 1 10 L 8 2 L 15 10";
                        stroke: #d5c4a1;
                        stroke-width: 2.4px;
                        stroke-line-cap: round;
                        stroke-line-join: round;
                    }

                    maximize-touch := TouchArea {
                        mouse-cursor: pointer;
                        clicked => {
                            root.window-state-change();
                            root.maximized = !root.maximized;
                        }
                    }
                }

                Rectangle {
                    x: island.width - 38px;
                    y: 1px;
                    width: 38px;
                    height: island.height - 2px;
                    border-radius: 10px;
                    background: close-touch.pressed
                        ? rgb(100, 45, 38)
                        : close-touch.has-hover ? rgb(80, 40, 35) : transparent;

                    Path {
                        x: 10px;
                        y: 8px;
                        width: 18px;
                        height: 18px;
                        commands: "M 9 1 L 9 8 M 3.3 3.7 A 7 7 0 1 0 14.7 3.7";
                        stroke: close-touch.has-hover ? #fb4934 : #fe8019;
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

fn load_background_path(path: &std::path::Path) -> Result<Image> {
    let decoded = image::open(path)
        .with_context(|| format!("No se pudo cargar la imagen de fondo {}", path.display()))?
        .into_rgba8();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        decoded.as_raw(),
        decoded.width(),
        decoded.height(),
    );
    Ok(Image::from_rgba8(buffer))
}

fn configured_background_path(
    paths: &AppPaths,
    appearance: &TerminalAppearance,
) -> Option<std::path::PathBuf> {
    if appearance.background_mode == "off" {
        return None;
    }
    let raw = appearance.background_image.trim();
    if raw.is_empty() {
        return None;
    }
    let configured = std::path::PathBuf::from(raw);
    Some(if configured.is_absolute() {
        configured
    } else {
        paths.root_dir().join(configured)
    })
}

fn load_background_image(paths: &AppPaths, appearance: &TerminalAppearance) -> Result<Option<Image>> {
    configured_background_path(paths, appearance)
        .map(|path| load_background_path(&path))
        .transpose()
}

struct BackgroundCarousel {
    images: Vec<std::path::PathBuf>,
    index: usize,
    last_change: Instant,
    last_raw: Option<bool>,
    last_window_state: Option<(bool, bool)>,
}

impl BackgroundCarousel {
    fn new(paths: &AppPaths, appearance: &TerminalAppearance) -> Result<Self> {
        let images = paths.background_images()?;
        let configured_name = configured_background_path(paths, appearance)
            .and_then(|path| path.file_name().map(|name| name.to_owned()));
        let index = configured_name
            .as_ref()
            .and_then(|name| {
                let wanted = name.to_string_lossy();
                images.iter().position(|path| {
                    path.file_name()
                        .and_then(|candidate| candidate.to_str())
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&wanted))
                })
            })
            .unwrap_or(0);

        Ok(Self {
            images,
            index,
            last_change: Instant::now(),
            last_raw: None,
            last_window_state: None,
        })
    }

    fn current_path(&self) -> Option<&std::path::Path> {
        self.images.get(self.index).map(std::path::PathBuf::as_path)
    }

    fn observe(
        &mut self,
        raw: bool,
        window_state: Option<(bool, bool)>,
        interval: Duration,
    ) -> bool {
        let raw_changed = self
            .last_raw
            .replace(raw)
            .is_some_and(|previous| previous != raw);

        let window_changed = match window_state {
            Some(state) => self
                .last_window_state
                .replace(state)
                .is_some_and(|previous| previous != state),
            None => false,
        };

        let timed = self.last_change.elapsed() >= interval;
        self.images.len() > 1 && (raw_changed || window_changed || timed)
    }

    fn rotate(&mut self) -> Option<std::path::PathBuf> {
        if self.images.is_empty() {
            return None;
        }
        if self.images.len() > 1 {
            self.index = (self.index + 1) % self.images.len();
        }
        self.last_change = Instant::now();
        self.current_path().map(std::path::Path::to_path_buf)
    }
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

const HISTORY_ROWS:usize=10_000;

/// The parser owns scrollback; a report owns only its reading position.
/// Keep the actual top row of a completed result, never clear the transcript.
struct ReadingAnchor { start_row:usize, history:usize, advances:usize }
fn history_len(parser:&mut vt100::Parser)->usize{
    let old=parser.screen().scrollback();
    parser.screen_mut().set_scrollback(usize::MAX);
    let len=parser.screen().scrollback();
    parser.screen_mut().set_scrollback(old);
    len
}
impl ReadingAnchor{
    fn begin(parser:&mut vt100::Parser)->Self{
        Self{start_row:usize::from(parser.screen().cursor_position().0),history:history_len(parser),advances:0}
    }
    fn observe(&mut self,bytes:&[u8],cols:u16){
        self.advances=self.advances.saturating_add(crate::presentation::formatter::Formatter::visual_row_advances(bytes,cols));
    }
    fn finish(self,parser:&mut vt100::Parser){
        let history=history_len(parser);
        let mut scrolled=history.saturating_sub(self.history);
        if history>=HISTORY_ROWS{
            let end=usize::from(parser.screen().cursor_position().0);
            scrolled=scrolled.max(self.start_row.saturating_add(self.advances).saturating_sub(end));
        }
        // Short output stays where it is. Long output opens on its first row.
        let offset=scrolled.saturating_sub(self.start_row).min(history);
        if offset>0{parser.screen_mut().set_scrollback(offset);}
    }
}

struct TerminalModel {
    session: EmbeddedSession,
    parser: vt100::Parser,
    reading_anchor:Option<ReadingAnchor>,
    font: Font,
    glyphs: HashMap<(char, u16), Glyph>,
    selection: Option<(usize, usize)>,
    selection_anchor: Option<usize>,
    dragging: bool,
    last_click: Option<(Instant, usize)>,
    click_count: u8,
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
            parser: vt100::Parser::new(INITIAL_ROWS, INITIAL_COLS, HISTORY_ROWS),
            reading_anchor:None,
            font,
            glyphs: HashMap::new(),
            selection: None,
            selection_anchor: None,
            dragging: false,
            last_click: None,
            click_count: 0,
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
        // Force a geometry recalculation because density/spacing can change.
        self.width = 0;
        self.height = 0;
        self.glyphs.clear();
        self.dirty = true;
    }

    fn input(&mut self, bytes: &[u8]) {
        self.reading_anchor=None;
        self.parser.screen_mut().set_scrollback(0);
        self.selection = None;
        self.selection_anchor = None;
        self.blink = Instant::now();
        self.dirty = true;
        let _ = self.session.write(bytes);
    }

    fn key_event(&mut self, code: CtKeyCode, modifiers: CtKeyModifiers) {
        self.reading_anchor=None;
        self.parser.screen_mut().set_scrollback(0);
        self.selection = None;
        self.selection_anchor = None;
        self.blink = Instant::now();
        self.dirty = true;
        let _ = self.session.send_key_event(CtKeyEvent::new(code, modifiers));
    }

    fn paste(&mut self, text: &str) {
        self.reading_anchor=None;
        self.parser.screen_mut().set_scrollback(0);
        self.selection = None;
        self.selection_anchor = None;
        self.blink = Instant::now();
        self.dirty = true;
        let _ = self.session.paste(text);
    }

    fn has_selection(&self) -> bool {
        self.selection.is_some_and(|(start, end)| end > start)
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
                if index < start || index >= end {
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
        let pad_x = (self.appearance.terminal_padding_x as f32 * scale).round();
        let pad_y = (self.appearance.terminal_padding_y as f32 * scale).round();
        let top_pad = ((ISLAND_TOP
            + ISLAND_HEIGHT
            + self.appearance.content_top_gap as f32)
            * scale)
            .round()
            + pad_y;

        (
            pad_x,
            top_pad,
            (self.appearance.cell_width as f32 * scale).round().max(1.0),
            (self.appearance.cell_height as f32 * scale).round().max(1.0),
        )
    }

    fn resize(&mut self, width: u32, height: u32, scale: f32) {
        // Defensive guard, even for call paths outside the window-state timer.
        if !valid_terminal_viewport(width, height, scale, false) {
            return;
        }
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
        let bottom_pad = (self.appearance.terminal_padding_y as f32 * scale).round();
        let rows = (((self.height as f32 - top_pad - bottom_pad) / cell_height).floor() as i32)
            .clamp(2, 200) as u16;

        if self.parser.screen().size() != (rows, cols) {
            self.reading_anchor=None;
            self.parser.screen_mut().set_size(rows, cols);
            self.session.resize(cols, rows);
            self.selection = None;
            self.selection_anchor = None;
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
        // The background manifest check can alert the user without pausing
        // startup or overwriting an in-progress command line.
        self.session.poll_update_notification();
        while let Ok(packet) = self.session.output.try_recv() {
            match packet {
                DisplayPacket::Report(bytes)=>{
                    // Do not steal a view the user is already inspecting.
                    if self.parser.screen().scrollback()==0 {
                        self.reading_anchor=Some(ReadingAnchor::begin(&mut self.parser));
                    }
                    let cols=self.parser.screen().size().1;
                    if let Some(anchor)=self.reading_anchor.as_mut(){anchor.observe(&bytes,cols);}
                    self.parser.process(&bytes);
                }
                DisplayPacket::Bytes(bytes)=>{
                    let cols=self.parser.screen().size().1;
                    if let Some(anchor)=self.reading_anchor.as_mut(){anchor.observe(&bytes,cols);}
                    self.parser.process(&bytes);
                }
                DisplayPacket::ReportEnd=>{
                    if let Some(anchor)=self.reading_anchor.take(){anchor.finish(&mut self.parser);}
                }
            }
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

    fn cell_text_at(&self, index: usize) -> String {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        if cols == 0 {
            return String::new();
        }
        let row = index / cols as usize;
        let col = index % cols as usize;
        if row >= rows as usize {
            return String::new();
        }
        screen
            .cell(row as u16, col as u16)
            .map(|cell| cell.contents().to_owned())
            .unwrap_or_default()
    }

    fn smart_cell_class(&self, index: usize) -> u8 {
        let text = self.cell_text_at(index);
        let Some(ch) = text.chars().next() else {
            return 0;
        };
        if ch.is_whitespace() {
            0
        } else if ch.is_alphanumeric()
            || ch == '_'
            || ".-/:\\@%+?=&~#$[]{}".contains(ch)
        {
            1
        } else {
            2
        }
    }

    fn word_bounds(&self, index: usize) -> (usize, usize) {
        let (_, cols) = self.parser.screen().size();
        if cols == 0 {
            return (index, index);
        }
        let cols = cols as usize;
        let row_start = (index / cols) * cols;
        let row_end = row_start + cols - 1;
        let class = self.smart_cell_class(index);
        if class == 0 {
            return (index, index);
        }

        let mut start = index;
        while start > row_start && self.smart_cell_class(start - 1) == class {
            start -= 1;
        }
        let mut end = index;
        while end < row_end && self.smart_cell_class(end + 1) == class {
            end += 1;
        }
        (start, end)
    }

    fn line_bounds(&self, index: usize) -> (usize, usize) {
        let (_, cols) = self.parser.screen().size();
        if cols == 0 {
            return (index, index);
        }
        let cols = cols as usize;
        let start = (index / cols) * cols;
        let mut end = start + cols - 1;
        while end > start && self.cell_text_at(end).trim().is_empty() {
            end -= 1;
        }
        (start, end)
    }

    fn pointer_down(&mut self, x: f32, y: f32) {
        let index = self.cell_at_logical(x, y);
        if self.session.raw_mode() && self.parser.screen().mouse_protocol_mode() == vt100::MouseProtocolMode::Press {
            let cols = usize::from(self.parser.screen().size().1).max(1);
            let _ = self.session.click_cell((index % cols) as u16,(index / cols) as u16);
            self.selection = None;
            self.selection_anchor = None;
            self.dragging = false;
            self.dirty = true;
            return;
        }
        let now = Instant::now();
        let repeated = self.last_click.is_some_and(|(when, previous)| {
            now.duration_since(when) <= Duration::from_millis(450)
                && previous.abs_diff(index) <= 1
        });
        self.click_count = if repeated {
            self.click_count.saturating_add(1).min(3)
        } else {
            1
        };
        self.last_click = Some((now, index));

        self.selection = match self.click_count {
            2 => {
                let (start, end) = self.word_bounds(index);
                Some((start, end.saturating_add(1)))
            }
            3 => {
                let (start, end) = self.line_bounds(index);
                Some((start, end.saturating_add(1)))
            }
            _ => Some((index, index.saturating_add(1))),
        };
        self.dragging = self.click_count == 1;
        self.selection_anchor = self.dragging.then_some(index);
        self.dirty = true;
    }

    fn pointer_move(&mut self, x: f32, y: f32) {
        if !self.dragging {
            return;
        }
        let index = self.cell_at_logical(x, y);
        if let Some(anchor) = self.selection_anchor {
            let (start, end) = if index >= anchor {
                (anchor, index.saturating_add(1))
            } else {
                (index, anchor.saturating_add(1))
            };
            self.selection = Some((start, end));
            self.dirty = true;
        }
    }

    fn pointer_up(&mut self) -> bool {
        self.dragging = false;
        self.selection_anchor = None;
        self.copy_selection()
    }

    fn copy_selection(&self) -> bool {
        if !self.has_selection() {
            return false;
        }
        let selected = self.selected_text();
        if selected.is_empty() {
            return false;
        }

        // Windows can keep the clipboard locked very briefly (for example by
        // clipboard history, RDP or another terminal). A single arboard attempt
        // made copy look randomly broken. Retry a few times before giving up.
        for delay_ms in [0_u64, 4, 12, 24] {
            if delay_ms != 0 {
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
            if let Ok(mut clipboard) = Clipboard::new()
                && clipboard.set_text(selected.clone()).is_ok()
            {
                return true;
            }
        }
        false
    }

    fn scroll(&mut self, delta_y: f32) {
        if delta_y.abs() < 0.5 {
            return;
        }
        let rows = (delta_y / 40.0).round() as i32;
        self.scroll_rows(rows);
    }

    fn scroll_rows(&mut self, rows:i32){
        self.reading_anchor=None;
        let current = self.parser.screen().scrollback() as i32;
        self.parser
            .screen_mut()
            .set_scrollback((current + rows).max(0) as usize);
        self.selection = None;
        self.selection_anchor = None;
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
            let selection = self.selection;
            let (left_pad, top_pad, cell_width, cell_height) = self.geometry();
            let font_px = (self.appearance.font_size as f32 * self.scale).round().max(8.0);
            let palette = self.appearance.font_palette.as_str();
            let default_fg = parse_rgb(&self.appearance.font_color).unwrap_or(FG);
            let default_fg = if palette == "gruvbox" { default_fg } else { font_palette_color(FG,palette) };

            let selected_row = selection.and_then(|(a,b)| (cols != 0 && a / cols as usize == b.saturating_sub(1) / cols as usize).then(|| a / cols as usize));
            for row in 0..rows {
                use crate::presentation::formatter::{Role,font_pixels};
                // A native visual accent appears only for a semantic title.
                // No ANSI boxes or graphical decoration are emitted by commands.
                let title= (0..cols.min(12)).any(|col|screen.cell(row,col).is_some_and(|cell|
                    cell.bold()&&cell.contents().chars().any(char::is_alphabetic)&&
                    matches!(cell.fgcolor(),vt100::Color::Rgb(r,g,b) if Role::from_rgb((r,g,b)).is_some_and(|role| role.style().section))));
                // A semantic card reserves the preceding band for title ascenders.
                // Render the actual glyph run at display size after painting its cells.
                let is_band = |r:u16| screen.cell(r,1).is_some_and(|cell|
                    matches!(cell.bgcolor(),vt100::Color::Rgb(r,g,b) if (r,g,b)==crate::presentation::formatter::REPORT_HEADER_RGB));
                let card_title = title && row > 0 && selection.is_none() && selected_row!=Some(usize::from(row)) && is_band(row) && is_band(row-1)
                    && (0..cols).all(|col|screen.cell(row-1,col).is_none_or(|c|c.contents().trim().is_empty()));
                let feature_color=|r:u16,c:u16|screen.cell(r,c).and_then(|cell|match cell.bgcolor(){
                    vt100::Color::Rgb(a,b,d) if (a,b,d)==crate::presentation::formatter::FEATURE_RGB || (a,b,d)==crate::presentation::formatter::FEATURE_ALT_RGB=>Some((a,b,d)),
                    _=>None
                });
                let feature_row=row>0 && selection.is_none() && selected_row!=Some(usize::from(row))
                    && (0..cols).any(|col|feature_color(row,col).is_some()
                        && feature_color(row-1,col)==feature_color(row,col))
                    && (0..cols).all(|col|screen.cell(row-1,col).is_none_or(|c|c.contents().trim().is_empty()));
                for col in 0..cols {
                    let Some(cell) = screen.cell(row, col) else {
                        continue;
                    };
                    if cell.is_wide_continuation() {
                        continue;
                    }

                    let mut fg = terminal_color(cell.fgcolor(), default_fg);
                    let mut bg = terminal_color(cell.bgcolor(), BG);
                    let layout_marker=matches!(cell.bgcolor(),vt100::Color::Rgb(r,g,b) if
                        (r,g,b)==crate::presentation::formatter::REPORT_HEADER_RGB
                        || (r,g,b)==crate::presentation::formatter::FEATURE_RGB
                        || (r,g,b)==crate::presentation::formatter::FEATURE_ALT_RGB);
                    let mut paint_background =
                        (!matches!(cell.bgcolor(), vt100::Color::Default) && !layout_marker) || cell.inverse();

                    if cell.inverse() {
                        std::mem::swap(&mut fg, &mut bg);
                    }

                    let index = row as usize * cols as usize + col as usize;
                    if selected_row == Some(usize::from(row)) {
                        bg = Rgb(SELECTED_RGB.0,SELECTED_RGB.1,SELECTED_RGB.2);
                        paint_background = true;
                    }
                    if selection
                        .is_some_and(|(a, b)| index >= a.min(b) && index < a.max(b))
                    {
                        let tone = Role::from_rgb((fg.0,fg.1,fg.2));
                        let alert = matches!(tone,Some(Role::Failure | Role::Warning));
                        if !alert {
                            fg = BG;
                            let (r,g,b) = Role::Label.style().rgb;
                            bg = Rgb(r,g,b);
                        } else {
                            bg = BG;
                        }
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

                    if !selection.is_some_and(|(a,b)| index >= a.min(b) && index < a.max(b)) && !cell.inverse() { fg = font_palette_color(fg,palette); }
                    let content = cell.contents();
                    if content.is_empty() || card_title || (feature_row && feature_color(row,col).is_some()) {
                        continue;
                    }

                    let role=match cell.fgcolor(){vt100::Color::Rgb(r,g,b)=>Role::from_rgb((r,g,b)),_=>None};
                    let font_px=if role.is_some(){font_pixels(role,font_px,cell_width,cell_height)}else{font_px};
                    let font_key=font_px.round().max(1.0) as u16;
                    let font_px=font_key as f32;
                    let mut pen_x = x;
                    let baseline = y + (cell_height * 0.80).round() as i32;
                    for ch in content.chars() {
                        let key = (ch, font_key);
                        if !self.glyphs.contains_key(&key) {
                            let (metrics, alpha) = self.font.rasterize(ch, font_px);
                            self.glyphs.insert(key, Glyph { metrics, alpha });
                        }
                        if let Some(glyph) = self.glyphs.get(&key) {
                            let centered=if role.is_some(){pen_x+((cell_width*wide-glyph.metrics.advance_width).max(0.0)*0.5).round() as i32}else{pen_x};
                            draw_glyph(
                                pixels,
                                width,
                                height,
                                centered,
                                baseline,
                                glyph,
                                fg,
                            );
                            if role.map_or(cell.bold(),|r|r.style().bold) {
                                draw_glyph(
                                    pixels,
                                    width,
                                    height,
                                    centered + self.scale.max(1.0).round() as i32,
                                    baseline,
                                    glyph,
                                    fg,
                                );
                            }
                            pen_x += glyph.metrics.advance_width.round() as i32;
                        }
                    }
                }
                if feature_row {
                    let mut col=0;
                    while col<cols{
                        let Some(surface)=feature_color(row,col)else{col+=1;continue;};
                        let start=col;
                        while col<cols && feature_color(row,col)==Some(surface){col+=1;}
                        let end=col;
                        let text=(start..end).filter_map(|c|screen.cell(row,c)).map(|c|c.contents()).collect::<String>();
                        let text=text.trim();
                        if text.is_empty(){continue;}
                        let color=(start..end).filter_map(|c|screen.cell(row,c)).find(|c|!c.contents().trim().is_empty())
                            .map(|c|font_palette_color(terminal_color(c.fgcolor(),default_fg),palette)).unwrap_or(default_fg);
                        let available=(f32::from(end-start)-3.).max(1.)*cell_width;
                        let size=(font_px*1.65).min(available/(0.62*text.chars().count().max(1) as f32))
                            .min(cell_height*1.6).round().max(1.) as u16;
                        let mut x=(left_pad+(f32::from(start)+2.)*cell_width).round() as i32;
                        let baseline=(top_pad+(f32::from(row)+0.56)*cell_height).round() as i32;
                        for ch in text.chars(){
                            let glyph=self.glyphs.entry((ch,size)).or_insert_with(||{
                                let (metrics,alpha)=self.font.rasterize(ch,f32::from(size));Glyph{metrics,alpha}
                            });
                            draw_glyph(pixels,width,height,x,baseline,glyph,color);
                            x+=glyph.metrics.advance_width.round() as i32;
                        }
                    }
                }
                if card_title {
                    let heading = (0..cols).filter_map(|col|screen.cell(row,col))
                        .map(|cell|cell.contents()).collect::<String>();
                    let heading = heading.trim();
                    let available = (f32::from(cols)-4.).max(1.)*cell_width;
                    let size = (font_px*1.7).min(available/(0.62*heading.chars().count().max(1) as f32))
                        .min(cell_height*1.6).round().max(1.) as u16;
                    let mut x = (left_pad+2.*cell_width).round() as i32;
                    let baseline = (top_pad+(f32::from(row)+0.48)*cell_height).round() as i32;
                    let (r,g,b) = Role::Title.style().rgb;
                    let Rgb(r,g,b) = font_palette_color(Rgb(r,g,b),palette);
                    for ch in heading.chars() {
                        let glyph = self.glyphs.entry((ch,size)).or_insert_with(||{
                            let (metrics,alpha) = self.font.rasterize(ch,f32::from(size));
                            Glyph { metrics,alpha }
                        });
                        draw_glyph(pixels,width,height,x,baseline,glyph,Rgb(r,g,b));
                        draw_glyph(pixels,width,height,x+1,baseline,glyph,Rgb(r,g,b));
                        x += glyph.metrics.advance_width.round() as i32;
                    }
                }
            }
        }

        self.dirty = false;
        Image::from_rgba8_premultiplied(buffer)
    }
}

// Central SST Gruvbox color policy. Legacy commands may still emit cyan or
// bright blue ANSI values; neutralize those at the terminal rendering boundary.
// Semantic red/amber/green and the muted blue used for explicitly optimal
// states remain distinguishable.
fn gruvbox_terminal_color(color: Rgb) -> Rgb {
    // Brand colors from assets/sst-icon.png are reserved for the SST logo.
    // Do not reinterpret them as blue UI chrome: the rest of the terminal
    // continues to follow the Gruvbox palette.
    if matches!(
        color,
        Rgb(35, 52, 106) | Rgb(54, 76, 119)
            | Rgb(109, 143, 171) | Rgb(186, 189, 198)
            | Rgb(6, 48, 185) | Rgb(17, 30, 70) | Rgb(251, 242, 54)
    ) {
        return color;
    }
    use crate::presentation::formatter::{Role, DEFAULT_FOREGROUND, DETAIL_RGB, LABEL_RGB};
    let themed = |(r,g,b)| Rgb(r,g,b);
    let Rgb(r,g,b) = color;
    if Role::from_rgb((r,g,b)).is_some() { return color; }
    match (r,g,b) {
        (240,242,244) => themed(Role::Title.style().rgb),
        (218,223,228) => themed(Role::Command.style().rgb),
        (223,227,232) => themed(DEFAULT_FOREGROUND),
        (185,191,199) | (213,196,161) => themed(DETAIL_RGB),
        (163,172,182) | (189,174,147) | (168,153,132) => themed(LABEL_RGB),
        _ if b >= 120 && b.saturating_add(5) >= g && b <= g.saturating_add(48)
                && g > r.saturating_add(18) => themed(LABEL_RGB),
        _ if b > 135 && b > r.saturating_add(28) && b > g.saturating_add(12) => themed(DETAIL_RGB),
        _ => color,
    }
}

// Recolor semantic foregrounds at the last rendering boundary, including
// existing scrollback. Keep the original semantic RGB for role recognition.
fn font_palette_color(color: Rgb, palette: &str) -> Rgb {
    use crate::presentation::formatter::Role;
    let Some(role) = Role::from_rgb((color.0,color.1,color.2)) else { return color; };
    let rgb = match (palette,role) {
        ("sunset", Role::Title | Role::Command) => (254,128,5),
        ("sunset", Role::Value | Role::Detail) => (251,186,118),
        // Contrast-safe tint of Stone Blue; never use Downriver as small text.
        ("sunset", Role::Label) => (157,167,181),
        ("sunset", Role::Warning) => (249,76,31),
        ("sunset", Role::Failure) => (206,47,37),
        ("golden", Role::Title | Role::Command | Role::Value | Role::Detail) => (227,212,115),
        // Contrast-safe tint of Pine Teal for the secondary typographic role.
        ("golden", Role::Label) => (139,180,172),
        ("golden", Role::Failure) => (215,46,46),
        ("golden", Role::Warning) => (227,212,115),
        _ => return color,
    };
    Rgb(rgb.0,rgb.1,rgb.2)
}

fn terminal_color(value: vt100::Color, default: Rgb) -> Rgb {
    const COLORS: [Rgb; 16] = [
        Rgb(0x28, 0x28, 0x28),
        Rgb(0xCC, 0x24, 0x1D),
        Rgb(0x98, 0x97, 0x1A),
        Rgb(0xD7, 0x99, 0x21),
        Rgb(0x45, 0x85, 0x88),
        Rgb(0xB1, 0x62, 0x86),
        Rgb(0x68, 0x9D, 0x6A),
        Rgb(0xA8, 0x99, 0x84),
        Rgb(0x92, 0x83, 0x74),
        Rgb(0xFB, 0x49, 0x34),
        Rgb(0xB8, 0xBB, 0x26),
        Rgb(0xFA, 0xBD, 0x2F),
        Rgb(0x83, 0xA5, 0x98),
        Rgb(0xD3, 0x86, 0x9B),
        Rgb(0x8E, 0xC0, 0x7C),
        Rgb(0xEB, 0xDB, 0xB2),
    ];

    match value {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => gruvbox_terminal_color(Rgb(r, g, b)),
        vt100::Color::Idx(i) if i < 16 => gruvbox_terminal_color(COLORS[i as usize]),
        vt100::Color::Idx(i) if i >= 232 => {
            let v = 8 + (i - 232) * 10;
            Rgb(v, v, v)
        }
        vt100::Color::Idx(i) => {
            let i = i - 16;
            let component = |n| if n == 0 { 0 } else { 55 + n * 40 };
            gruvbox_terminal_color(Rgb(component(i / 36), component(i / 6 % 6), component(i % 6)))
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

    // Slint entrega las teclas modificadoras como eventos con un código Unicode
    // privado en event.text. No son entrada de terminal. Si se reenvían, Ctrl
    // por sí solo borra la selección y el PTY llega a mostrar ese código.
    // Ignorarlas preserva la selección hasta que llegue la tecla real (C, V, etc.).
    if [
        Key::Shift,
        Key::ShiftR,
        Key::Control,
        Key::ControlR,
        Key::Alt,
        Key::AltGr,
        Key::Meta,
        Key::MetaR,
        Key::CapsLock,
    ]
    .into_iter()
    .any(|key| key_is(text, key))
    {
        return;
    }

    if ctrl && text.eq_ignore_ascii_case("q") {
        model.session.force_abort();
        model.dirty = true;
        return;
    }

    let copy_key = text.eq_ignore_ascii_case("c");
    let control_c = text == "\u{3}";

    if model.has_selection() && ((ctrl && copy_key) || control_c) {
        let _ = model.copy_selection();
        return;
    }

    if ctrl && shift && copy_key && model.has_selection() {
        let _ = model.copy_selection();
        return;
    }

    // Slint/Windows may deliver Ctrl+C as ETX (0x03) without control=true.
    // With no selection it must remain a terminal interrupt.
    if control_c && !model.has_selection() {
        if model.session.raw_mode() {
            let _ = model.session.send_raw_key(CtKeyEvent::new(
                CtKeyCode::Char('c'),
                CtKeyModifiers::CONTROL,
            ));
            model.parser.screen_mut().set_scrollback(0);
            model.selection = None;
            model.selection_anchor = None;
            model.dirty = true;
        } else {
            model.input(b"\x03");
        }
        return;
    }

    if ctrl && key_is(text, Key::Insert) && model.has_selection() {
        let _ = model.copy_selection();
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
            model.selection_anchor = None;
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
        model.scroll_rows(i32::from(model.parser.screen().size().0.saturating_sub(2).max(1)));
    } else if key_is(text, Key::PageDown) {
        model.scroll_rows(-i32::from(model.parser.screen().size().0.saturating_sub(2).max(1)));
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
        // Slint's no-frame and winit's undecorated flags may not remove
        // the residual Win32 caption buttons after a native state transition.
        // Keep the OS resize border; remove only the system-drawn caption so
        // the SST island is the single visible set of window controls.
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        if style & WS_CAPTION != 0 {
            SetWindowLongW(hwnd, GWL_STYLE, (style & !WS_CAPTION) as i32);
            SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE
                | SWP_NOZORDER | SWP_NOACTIVATE);
        }
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

        apply_configured_backdrop(hwnd, appearance, true);
        apply_native_corner_preference(hwnd, appearance);

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

// Expose the verified token level to the OS task switcher as well.
/// Slint/winit's undecorated HWND can retain its generic window icon even
/// when sst.exe contains RT_GROUP_ICON. Bind the embedded Win32 resource
/// explicitly for the taskbar, Alt+Tab and the native window identity.
fn apply_embedded_sst_icon(hwnd: HWND) {
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        if module.is_null() {
            return;
        }
        // winres::WindowsResource::set_icon stores the application icon as
        // resource ID 1. This loads the existing official .ico, never a
        // generated/placeholder visual or a separate on-disk resource.
        let icon = LoadIconW(module, 1usize as *const u16);
        if !icon.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon as isize);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icon as isize);
            // Keep the class fallback in sync for Explorer/Alt+Tab and
            // for frameless windows whose explicit icon is reset by winit.
            SetClassLongPtrW(hwnd,GCLP_HICON,icon as isize);
            SetClassLongPtrW(hwnd,GCLP_HICONSM,icon as isize);
        }
    }
}

fn set_native_privilege_title(hwnd: HWND, privilege_label: &str) {
    let title: Vec<u16> = format!("Shell Shock Tool [{}]", privilege_label)
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe { SetWindowTextW(hwnd, title.as_ptr()); }
}

unsafe fn focus_native_window(hwnd: HWND) {
    unsafe {
        let foreground = GetForegroundWindow();
        let current_thread = GetCurrentThreadId();
        let foreground_thread = if foreground.is_null() {
            0
        } else {
            GetWindowThreadProcessId(foreground, std::ptr::null_mut())
        };

        let attached = foreground_thread != 0
            && foreground_thread != current_thread
            && AttachThreadInput(current_thread, foreground_thread, 1) != 0;

        // Slint's FocusScope may already be focused internally while Windows
        // still considers another native window active. Restore/raise the HWND
        // first, then establish native keyboard focus.
        ShowWindow(hwnd, SW_RESTORE);
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        SetActiveWindow(hwnd);
        SetFocus(hwnd);

        if attached {
            AttachThreadInput(current_thread, foreground_thread, 0);
        }
    }
}

unsafe fn apply_native_corner_preference(
    hwnd: HWND,
    appearance: &TerminalAppearance,
) {
    unsafe {
        // DWMWA_WINDOW_CORNER_PREFERENCE = 33.
        // 1 = DWMWCP_DONOTROUND, 2 = DWMWCP_ROUND.
        // Windows owns the actual window shape, maximize/restore behavior and shadow.
        let preference: i32 = if appearance.corner_radius == 0 { 1 } else { 2 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            33,
            (&preference as *const i32).cast(),
            size_of::<i32>() as u32,
        );
    }
}

fn apply_slint_window_effects(ui: &SstBlueWindow, appearance: &TerminalAppearance) {
    if let Some(hwnd) = slint_hwnd(ui) {
        unsafe { apply_window_effects(hwnd, appearance) };
    }
}

pub fn run() -> Result<()> {
    // The Winit/FemtoVG OpenGL failure is discovered too late for Slint's
    // automatic renderer fallback on Windows. Always select software rendering
    // for SST's GUI; no GPU or OpenGL context is needed for terminal startup.
    // See upstream slint-ui/slint#8708 for this Windows regression.
    BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attributes| {
            attributes
                .with_transparent(true)
                .with_decorations(false)
        })
        .select()
        .map_err(|error| anyhow::anyhow!("No se pudo inicializar la interfaz Winit/software de SST: {error}"))?;

    let paths = AppPaths::detect();
    paths.ensure_layout()?;
    let appearance = paths.load_appearance()?;
    let initial_carousel = BackgroundCarousel::new(&paths, &appearance)?;
    let background_image = if appearance.background_mode == "carrousel" {
        initial_carousel
            .current_path()
            .map(load_background_path)
            .transpose()?
    } else {
        load_background_image(&paths, &appearance)?
    };
    let carousel_state = Rc::new(RefCell::new(initial_carousel));
    let carousel_window_event = Rc::new(Cell::new(false));
    let appearance_state = Rc::new(RefCell::new(appearance.clone()));
    let model = Rc::new(RefCell::new(TerminalModel::new(appearance.clone())?));
    let metrics = Rc::new(RefCell::new(System::new()));
    let ui = SstBlueWindow::new()?;
    let color = |(r,g,b)| slint::Color::from_rgb_u8(r,g,b);
    let theme = ui.global::<OutputTheme>();
    theme.set_rail(color(crate::presentation::formatter::SECTION_RAIL));
    theme.set_rule(color(crate::presentation::formatter::SECTION_RULE));

    // The sudo command starts a NEW Windows process for each privilege
    // escalation. Its token is immutable during this GUI session. Query that
    // token directly once and retain the visible indicator for the window's
    // entire lifetime, including unfocused and restored states.
    let privilege_label = match crate::builtins::gui_privilege_indicator() {
        Ok(label) => label.to_owned(),
        Err(error) => {
            eprintln!("No se pudo determinar el token de privilegios de SST: {error}");
            "TOKEN ?".to_owned()
        }
    };
    ui.set_privilege_label(privilege_label.clone().into());

    if let Some(image) = background_image {
        ui.set_background_image(image);
    }
    ui.set_background_image_opacity(background_image_opacity(&appearance, true));
    ui.set_background_image_fit_mode(appearance.background_image_fit.clone().into());

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
                if !model.copy_selection()
                    && let Ok(mut clipboard) = Clipboard::new()
                    && let Ok(value) = clipboard.get_text()
                {
                    model.paste(&value);
                }
            } else {
                let _ = model.pointer_up();
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
        let carousel_window_event = carousel_window_event.clone();
        ui.on_window_state_change(move || {
            carousel_window_event.set(true);
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
        let privilege_label = privilege_label.clone();
        Timer::single_shot(Duration::ZERO, move || {
            if let Some(ui) = weak.upgrade() {
                apply_slint_window_effects(&ui, &appearance);
                if let Some(hwnd) = slint_hwnd(&ui) {
                    apply_embedded_sst_icon(hwnd);
                    set_native_privilege_title(hwnd, &privilege_label);
                    unsafe { focus_native_window(hwnd); }
                }
            }
        });
    }

    let weak = ui.as_weak();
    let last_status = Rc::new(RefCell::new(Instant::now() - Duration::from_secs(2)));
    // None forces native DWM state to be applied once after the window is fully shown.
    let last_focus = Rc::new(RefCell::new(None::<bool>));
    // The compositor may drop native composition state while the HWND is iconic.
    // Track the native transition instead of relying on the focus state alone.
    let last_minimized = Rc::new(Cell::new(false));
    let restore_viewport = Rc::new(RefCell::new(RestoreViewportGuard::default()));
    // Winit/Windows can ignore a single activation request while the HWND is
    // still being shown. Retry briefly during startup so SST accepts typing
    // immediately without requiring a mouse click.
    let startup_focus_attempts = Rc::new(Cell::new(30u8));
    let timer = Timer::default();
    {
        let model = model.clone();
        let metrics = metrics.clone();
        let last_status = last_status.clone();
        let last_focus = last_focus.clone();
        let last_minimized = last_minimized.clone();
        let restore_viewport = restore_viewport.clone();
        let startup_focus_attempts = startup_focus_attempts.clone();
        let appearance_state = appearance_state.clone();
        let carousel_state = carousel_state.clone();
        let carousel_window_event = carousel_window_event.clone();
        let paths = paths.clone();

        timer.start(TimerMode::Repeated, Duration::from_millis(33), move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };

            let size = ui.window().size();
            let scale = ui.window().scale_factor();
            let minimized = slint_hwnd(&ui).is_some_and(|hwnd| unsafe { IsIconic(hwnd) != 0 });
            let drawable = restore_viewport.borrow_mut().drawable(
                size.width,
                size.height,
                scale,
                minimized,
            );

            let reload_requested = {
                let model = model.borrow();
                model.session.take_config_reload_request()
            };

            if reload_requested {
                match paths.load_appearance()
                    .and_then(|next| {
                        let carousel = BackgroundCarousel::new(&paths, &next)?;
                        let image = if next.background_mode == "carrousel" {
                            carousel
                                .current_path()
                                .map(load_background_path)
                                .transpose()?
                        } else {
                            load_background_image(&paths, &next)?
                        };
                        Ok((next, image, carousel))
                    })
                {
                    Ok((next, image, carousel)) => {
                        let focused = slint_hwnd(&ui)
                            .is_some_and(|hwnd| unsafe { GetForegroundWindow() == hwnd });

                        ui.set_background_image(image.unwrap_or_default());
                        ui.set_background_image_opacity(
                            background_image_opacity(&next, focused)
                        );
                        ui.set_background_image_fit_mode(next.background_image_fit.clone().into());

                        model.borrow_mut().set_appearance(next.clone());
                        *appearance_state.borrow_mut() = next.clone();
                        *carousel_state.borrow_mut() = carousel;

                        if let Some(hwnd) = slint_hwnd(&ui) {
                            unsafe {
                                apply_configured_backdrop(hwnd, &next, focused);
                                apply_native_corner_preference(hwnd, &next);
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("No se pudo recargar la apariencia de SST: {error}");
                    }
                }
            }

            if let Some(hwnd) = slint_hwnd(&ui) {
                if drawable && unsafe{SendMessageW(hwnd,WM_GETICON,ICON_BIG as usize,0)==0
                    || SendMessageW(hwnd,WM_GETICON,ICON_SMALL as usize,0)==0}{
                    apply_embedded_sst_icon(hwnd);
                }
                if !drawable {
                    last_minimized.set(true);
                } else if last_minimized.replace(false) {
                    // On restore, Windows may have discarded the DWM accent and
                    // nonclient corner state without any foreground-focus change.
                    // Reapply the actual native effects and repaint the terminal.
                    let appearance = appearance_state.borrow().clone();
                    unsafe { apply_window_effects(hwnd, &appearance); }
                    model.borrow_mut().dirty = true;
                }
                let mut focused = drawable && unsafe { GetForegroundWindow() == hwnd };

                if drawable && !focused && startup_focus_attempts.get() > 0 {
                    unsafe { focus_native_window(hwnd); }
                    startup_focus_attempts.set(startup_focus_attempts.get().saturating_sub(1));
                    focused = unsafe { GetForegroundWindow() == hwnd };
                } else if focused {
                    startup_focus_attempts.set(0);
                }

                let appearance = appearance_state.borrow().clone();

                let mut previous = last_focus.borrow_mut();
                if previous.as_ref().copied() != Some(focused) {
                    *previous = Some(focused);
                    model.borrow_mut().set_focused(focused);
                    ui.set_background_image_opacity(background_image_opacity(&appearance, focused));
                    unsafe {
                        apply_configured_backdrop(hwnd, &appearance, focused);
                        apply_native_corner_preference(hwnd, &appearance);
                    }
                }
            }

            {
                let appearance = appearance_state.borrow().clone();
                let raw = model.borrow().session.raw_mode();
                let window_state = slint_hwnd(&ui).map(|hwnd| unsafe {
                    (IsIconic(hwnd) != 0, IsZoomed(hwnd) != 0)
                });

                if appearance.background_mode == "carrousel" {
                    let interval = Duration::from_secs(
                        appearance.background_carousel_minutes.saturating_mul(60),
                    );
                    let observed_change = carousel_state
                        .borrow_mut()
                        .observe(raw, window_state, interval);
                    let explicit_window_change = carousel_window_event.replace(false);
                    if observed_change || explicit_window_change {
                        if let Some(path) = carousel_state.borrow_mut().rotate() {
                            match load_background_path(&path) {
                                Ok(image) => ui.set_background_image(image),
                                Err(error) => {
                                    eprintln!(
                                        "No se pudo cargar fondo del carrusel {}: {error}",
                                        path.display()
                                    );
                                }
                            }
                        }
                    }
                } else {
                    carousel_window_event.set(false);
                    let _ = carousel_state
                        .borrow_mut()
                        .observe(raw, window_state, Duration::from_secs(u64::MAX));
                }
            }

            {
                let mut model = model.borrow_mut();
                // Windows may report a zero-sized client area while iconic.
                // Never turn a live PTY into a 2x2 terminal on minimize: doing
                // so can reflow or discard the rendered scrollback on restore.
                // Continue collecting PTY output, but keep its last good size.
                if drawable {
                    model.resize(size.width, size.height, scale);
                }
                model.tick();

                let screen = model.parser.screen();
                let show_caret = drawable && screen.scrollback() == 0 && !screen.hide_cursor();
                ui.set_caret_visible(show_caret);
                if show_caret {
                    let (row,col) = screen.cursor_position();
                    let (left,top,w,h) = model.geometry();
                    let factor = model.scale.max(0.5);
                    ui.set_caret_x((left+f32::from(col)*w)/factor);
                    ui.set_caret_y((top+f32::from(row)*h)/factor);
                    ui.set_caret_height((h/factor-1.0).max(1.0));
                    let phase = if model.focused { ((model.blink.elapsed().as_millis()/100)%16) as u8 } else { 0 };
                    let (r,g,b) = caret_color(phase);
                    ui.set_caret_tone(slint::Color::from_rgb_u8(r,g,b));
                }
                let panels = model.session.panels_snapshot();
                // Reports use typography, not arbitrary per-line vector boxes.
                // Dedicated live monitors retain their explicit section frames.
                ui.set_report_heading_count(0);
                ui.set_monitor_panel_count(panels.len().min(4) as i32);
                if !panels.is_empty() {
                    let (left, top, cell_width, cell_height) = model.geometry();
                    let factor = model.scale.max(0.5);
                    let (_, cols) = model.parser.screen().size();
                    ui.set_monitor_grid_left((left + 0.35 * cell_width) / factor);
                    ui.set_monitor_grid_top((top - 0.05 * cell_height) / factor);
                    ui.set_monitor_grid_width(
                        (cols.saturating_sub(1) as f32 * cell_width) / factor,
                    );
                    ui.set_monitor_line_height(cell_height / factor);
                    let mut iter = panels.iter();
                    let mut next = || iter.next().map(|p| (f32::from(p.start_row), f32::from(p.rows)))
                        .unwrap_or((0.0, 0.0));
                    let (row, rows) = next();
                    ui.set_monitor_a_row(row);
                    ui.set_monitor_a_rows(rows);
                    let (row, rows) = next();
                    ui.set_monitor_b_row(row);
                    ui.set_monitor_b_rows(rows);
                    let (row, rows) = next();
                    ui.set_monitor_c_row(row);
                    ui.set_monitor_c_rows(rows);
                    let (row, rows) = next();
                    ui.set_monitor_d_row(row);
                    ui.set_monitor_d_rows(rows);
                }

                // Shared transfer metrics are read by the Slint event loop.
                // No ANSI progress bar, global reading layer, or window texture
                // modification is used while downloading through instp.
                if let Some(progress) = crate::support::transfer::snapshot() {
                    if progress.finished {
                        // Completion/failure is written as a normal command
                        // line; remove the inline graphic immediately.
                        ui.set_transfer_visible(false);
                    } else {
                        // The worker writes: title + two reserved lines, so
                        // the native track goes in the first reserved row.
                        // It follows the terminal cursor when the view scrolls.
                        let (left, top, _, line_height) = model.geometry();
                        let (cursor_row, _) = model.parser.screen().cursor_position();
                        let graphic_row = cursor_row.saturating_sub(2);
                        let factor = scale.max(0.5);
                        ui.set_transfer_x((left / factor).max(8.0));
                        ui.set_transfer_y((top + f32::from(graphic_row) * line_height) / factor);
                        ui.set_transfer_visible(true);
                        ui.set_transfer_ratio(progress.fraction());
                        ui.set_transfer_known_total(progress.expected.is_some());
                        ui.set_transfer_percent(progress.percentage().into());
                        ui.set_transfer_amount(progress.amount().into());
                        ui.set_transfer_speed(progress.speed().into());
                        ui.set_transfer_eta(progress.remaining().into());
                    }
                } else if ui.get_transfer_visible() {
                    ui.set_transfer_visible(false);
                }

                if drawable && model.dirty && !ui.get_keygen_visible() {
                    ui.set_terminal_image(model.render());
                }

                if model.session.exited() {
                    let _ = ui.hide();
                    let _ = slint::quit_event_loop();
                    return;
                }
            }

            if last_status.borrow().elapsed() >= Duration::from_secs(2) {
                let mut system = metrics.borrow_mut();
                system.refresh_cpu_usage();
                system.refresh_memory();

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

    // Keygen frames are independent of the 33 ms terminal-maintenance timer.
    // The GUI receives the latest frame at 60 Hz with no queued stale frames.
    let keygen_timer=Timer::default();
    let keygen_weak=ui.as_weak();
    keygen_timer.start(TimerMode::Repeated,Duration::from_millis(16),move||{
        let Some(ui)=keygen_weak.upgrade() else{return;};
        use crate::application::system::{KEYGEN_FRAME,KeygenFrame};
        let frame=KEYGEN_FRAME.lock().unwrap_or_else(|e|e.into_inner()).take();
        match frame{
            Some(KeygenFrame::Draw(buffer))=>{
                if !slint_hwnd(&ui).is_some_and(|hwnd|unsafe{IsIconic(hwnd)!=0}){
                    ui.set_keygen_image(Image::from_rgba8(buffer));
                    if !ui.get_keygen_visible(){ui.set_keygen_visible(true);}
                }
            }
            Some(KeygenFrame::Closed)=>{
                ui.set_keygen_visible(false);
                ui.set_keygen_image(Image::default());
            }
            None=>{}
        }
    });
    slint::run_event_loop()?;
    keygen_timer.stop();
    timer.stop();
    let _ = ui.hide();
    Ok(())
}

#[cfg(test)]
mod reading_viewport_tests{
    use super::*;
    #[test]
    fn completed_report_opens_at_beginning_and_retains_prompt(){
        let mut parser=vt100::Parser::new(8,40,HISTORY_ROWS);
        parser.process(b"previous output\r\n$ sys\r\n");
        let mut anchor=ReadingAnchor::begin(&mut parser);
        let body=b"TITLE\r\nrow 1\r\nrow 2\r\nrow 3\r\nrow 4\r\nrow 5\r\nrow 6\r\nrow 7\r\nrow 8\r\n";
        anchor.observe(body,40);parser.process(body);
        parser.process(b"$ ");
        anchor.finish(&mut parser);
        assert!(parser.screen().contents().lines().next().unwrap().contains("TITLE"));
        parser.screen_mut().set_scrollback(0);
        assert!(parser.screen().contents().contains("$ "));
    }
    #[test]
    fn compact_output_does_not_jump(){
        let mut parser=vt100::Parser::new(8,40,HISTORY_ROWS);
        parser.process(b"$ sys\r\n");
        let mut anchor=ReadingAnchor::begin(&mut parser);
        anchor.observe(b"ok\r\n",40);parser.process(b"ok\r\n$ ");
        anchor.finish(&mut parser);
        assert_eq!(parser.screen().scrollback(),0);
    }
}

#[cfg(test)]
mod font_palette_regressions {
    use super::*;
    use crate::presentation::formatter::Role;
    fn role(r:Role)->Rgb { let (r,g,b)=r.style().rgb; Rgb(r,g,b) }
    #[test]
    fn palettes_change_foregrounds_without_reinterpreting_brand_colors(){
        let base=role(Role::Value);
        let s=font_palette_color(base,"sunset");
        let g=font_palette_color(base,"golden");
        assert_eq!((s.0,s.1,s.2),(251,186,118));
        assert_eq!((g.0,g.1,g.2),(227,212,115));
        let brand=font_palette_color(Rgb(35,52,106),"sunset");
        assert_eq!((brand.0,brand.1,brand.2),(35,52,106));
        let original=font_palette_color(base,"gruvbox");
        assert_eq!((original.0,original.1,original.2),(base.0,base.1,base.2));
    }
}
