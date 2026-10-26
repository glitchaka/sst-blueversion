use std::{
    collections::HashMap,
    ffi::c_void,
    mem::zeroed,
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CreateEllipticRgn, CreateRoundRectRgn, CreateSolidBrush, DeleteObject,
        DrawTextW, Ellipse, EndPaint, GetStockObject, InvalidateRect, PAINTSTRUCT, RoundRect,
        SelectObject, SetBkMode, SetTextColor, SetWindowRgn, DEFAULT_GUI_FONT,
        DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, TRANSPARENT,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows,
        GetClientRect, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, MSG, PM_REMOVE, PeekMessageW,
        PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
        SWP_NOOWNERZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
        WNDCLASSW, GWLP_USERDATA, HWND_TOP, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONUP,
        WM_PAINT, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
    },
};

use super::lan_beacon::{BeaconEvent, BeaconPeer, SstBeaconHandle};

const COLLAPSED_SIZE: i32 = 14;
const EXPANDED_WIDTH: i32 = 350;
const EXPANDED_HEIGHT: i32 = 54;
const EDGE_MARGIN: i32 = 18;
const CLASS_NAME: &[u16] = &[
    b'S' as u16, b'S' as u16, b'T' as u16, b'B' as u16, b'e' as u16, b'a' as u16,
    b'c' as u16, b'o' as u16, b'n' as u16, b'I' as u16, b's' as u16, b'l' as u16,
    b'a' as u16, b'n' as u16, b'd' as u16, 0,
];

pub struct BeaconOverlayGuard {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl BeaconOverlayGuard {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("sst-beacon-island".to_owned())
            .spawn(move || run_overlay(worker_stop))
            .ok();
        Self { stop, worker }
    }
}

impl Drop for BeaconOverlayGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct OverlayState {
    owner: HWND,
    beacon: Option<SstBeaconHandle>,
    enabled: bool,
    peers: HashMap<String, BeaconPeer>,
    peer_count: usize,
    selected: Option<String>,
    hwnd: HWND,
}

impl OverlayState {
    fn new(owner: HWND) -> Self {
        let beacon = SstBeaconHandle::start().ok();
        let enabled = beacon.is_some();
        Self {
            owner,
            beacon,
            enabled,
            peers: HashMap::new(),
            peer_count: 0,
            selected: None,
            hwnd: null_mut(),
        }
    }

    fn expanded(&self) -> bool {
        self.enabled && !self.peers.is_empty()
    }

    fn size(&self) -> (i32, i32) {
        if self.expanded() {
            (EXPANDED_WIDTH, EXPANDED_HEIGHT)
        } else {
            (COLLAPSED_SIZE, COLLAPSED_SIZE)
        }
    }

    fn selected_peer(&self) -> Option<&BeaconPeer> {
        self.selected
            .as_ref()
            .and_then(|id| self.peers.get(id))
            .or_else(|| self.peers.values().next())
    }

    fn process_events(&mut self) -> bool {
        let mut changed = false;
        let Some(beacon) = self.beacon.as_ref() else {
            return changed;
        };

        while let Some(event) = beacon.try_event() {
            match event {
                BeaconEvent::PeerJoined { peer, peer_count } => {
                    self.selected = Some(peer.id.clone());
                    self.peers.insert(peer.id.clone(), peer);
                    self.peer_count = peer_count;
                    self.enabled = true;
                    changed = true;
                }
                BeaconEvent::PeerLeft { peer, peer_count } => {
                    self.peers.remove(&peer.id);
                    self.peer_count = peer_count;
                    if self.selected.as_deref() == Some(peer.id.as_str()) {
                        self.selected = self.peers.keys().next().cloned();
                    }
                    changed = true;
                }
                BeaconEvent::Status { enabled, peer_count } => {
                    self.enabled = enabled;
                    self.peer_count = peer_count;
                    if !enabled {
                        self.peers.clear();
                        self.selected = None;
                    }
                    changed = true;
                }
            }
        }
        changed
    }

    fn toggle(&mut self) {
        let Some(beacon) = self.beacon.as_ref() else {
            self.enabled = false;
            self.peer_count = 0;
            return;
        };
        let next = !self.enabled;
        beacon.set_enabled(next);
        self.enabled = next;
        self.peers.clear();
        self.peer_count = 0;
        self.selected = None;
    }
}

fn run_overlay(stop: Arc<AtomicBool>) {
    let Some(owner) = wait_for_sst_window(&stop) else {
        return;
    };

    let mut state = Box::new(OverlayState::new(owner));
    let hwnd = unsafe { create_overlay_window(owner) };
    if hwnd.is_null() {
        return;
    }
    state.hwnd = hwnd;
    unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (&mut *state as *mut OverlayState).cast::<c_void>() as isize,
        );
    }

    let mut last_size = (0, 0);
    while !stop.load(Ordering::Acquire) {
        let changed = state.process_events();
        let size = state.size();
        let size_changed = size != last_size;
        if size_changed {
            last_size = size;
            unsafe { update_window_region(hwnd, size.0, size.1, state.expanded()) };
        }
        if changed || size_changed {
            unsafe {
                InvalidateRect(hwnd, null(), 1);
            }
        }
        unsafe {
            position_overlay(&state);
        }

        let mut message: MSG = unsafe { zeroed() };
        unsafe {
            while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        thread::sleep(Duration::from_millis(80));
    }

    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        DestroyWindow(hwnd);
    }
}

fn wait_for_sst_window(stop: &AtomicBool) -> Option<HWND> {
    while !stop.load(Ordering::Acquire) {
        if let Some(hwnd) = find_sst_window() {
            return Some(hwnd);
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

fn find_sst_window() -> Option<HWND> {
    struct Search {
        pid: u32,
        hwnd: HWND,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> i32 {
        let state = unsafe { &mut *(lparam as *mut Search) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid != state.pid || unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }

        let length = unsafe { GetWindowTextLengthW(hwnd) };
        if length <= 0 {
            return 1;
        }
        let mut title = vec![0u16; length as usize + 1];
        let copied = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
        if copied <= 0 {
            return 1;
        }
        let title = String::from_utf16_lossy(&title[..copied as usize]);
        if title == "Shell Shock Tool" {
            state.hwnd = hwnd;
            return 0;
        }
        1
    }

    let mut search = Search {
        pid: std::process::id(),
        hwnd: null_mut(),
    };
    unsafe {
        EnumWindows(Some(callback), (&mut search as *mut Search) as LPARAM);
    }
    (!search.hwnd.is_null()).then_some(search.hwnd)
}

unsafe fn create_overlay_window(owner: HWND) -> HWND {
    let instance = unsafe { GetModuleHandleW(null()) };
    let class = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: CLASS_NAME.as_ptr(),
    };
    unsafe {
        RegisterClassW(&class);
    }

    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS_NAME.as_ptr(),
            CLASS_NAME.as_ptr(),
            WS_POPUP,
            0,
            0,
            COLLAPSED_SIZE,
            COLLAPSED_SIZE,
            owner,
            null_mut(),
            instance,
            null(),
        )
    }
}

unsafe fn update_window_region(hwnd: HWND, width: i32, height: i32, expanded: bool) {
    let region = if expanded {
        unsafe { CreateRoundRectRgn(0, 0, width + 1, height + 1, 18, 18) }
    } else {
        unsafe { CreateEllipticRgn(0, 0, width + 1, height + 1) }
    };
    if region.is_null() {
        return;
    }
    if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
        unsafe { DeleteObject(region) };
    }
}

unsafe fn position_overlay(state: &OverlayState) {
    if unsafe { IsWindowVisible(state.owner) } == 0 || unsafe { IsIconic(state.owner) } != 0 {
        unsafe {
            ShowWindow(state.hwnd, SW_HIDE);
        }
        return;
    }

    let mut rect: RECT = unsafe { zeroed() };
    if unsafe { GetWindowRect(state.owner, &mut rect) } == 0 {
        return;
    }
    let (width, height) = state.size();
    let x = rect.right - width - EDGE_MARGIN;
    let y = rect.bottom - height - EDGE_MARGIN;
    unsafe {
        SetWindowPos(
            state.hwnd,
            HWND_TOP,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
        ShowWindow(state.hwnd, SW_SHOWNOACTIVATE);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
    } as *mut OverlayState;

    match message {
        WM_ERASEBKGND => 1,
        WM_LBUTTONUP => {
            if !state_ptr.is_null() {
                let state = unsafe { &mut *state_ptr };
                state.toggle();
                unsafe { InvalidateRect(hwnd, null(), 1) };
            }
            0
        }
        WM_PAINT => {
            if !state_ptr.is_null() {
                unsafe { paint(hwnd, &*state_ptr) };
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn paint(hwnd: HWND, state: &OverlayState) {
    let mut paint: PAINTSTRUCT = unsafe { zeroed() };
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if hdc.is_null() {
        return;
    }

    let mut rect: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut rect) };

    let dot_color = if !state.enabled {
        rgb(105, 115, 130)
    } else if state.peers.is_empty() {
        rgb(70, 160, 255)
    } else {
        rgb(255, 82, 92)
    };

    if !state.expanded() {
        let dot = unsafe { CreateSolidBrush(dot_color) };
        let old_dot = unsafe { SelectObject(hdc, dot) };
        unsafe {
            Ellipse(hdc, 0, 0, rect.right, rect.bottom);
            SelectObject(hdc, old_dot);
            DeleteObject(dot);
            EndPaint(hwnd, &paint);
        }
        return;
    }

    let bg = unsafe { CreateSolidBrush(rgb(10, 13, 20)) };
    let old_brush = unsafe { SelectObject(hdc, bg) };
    unsafe {
        RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 18, 18);
        SelectObject(hdc, old_brush);
        DeleteObject(bg);
    }

    let dot = unsafe { CreateSolidBrush(dot_color) };
    let old_dot = unsafe { SelectObject(hdc, dot) };
    unsafe {
        Ellipse(hdc, 13, 18, 31, 36);
        SelectObject(hdc, old_dot);
        DeleteObject(dot);
    }

    if let Some(peer) = state.selected_peer() {
        let extra = state.peer_count.saturating_sub(1);
        let label = if extra == 0 {
            format!("{}  ·  {}  ·  SST {}", peer.host, peer.address, peer.version)
        } else {
            format!(
                "{}  ·  {}  ·  SST {}   +{}",
                peer.host, peer.address, peer.version, extra
            )
        };
        let wide = label.encode_utf16().collect::<Vec<_>>();
        let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
        let old_font = unsafe { SelectObject(hdc, font) };
        unsafe {
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, rgb(223, 232, 239));
        }
        let mut text_rect = RECT {
            left: 42,
            top: 0,
            right: rect.right - 10,
            bottom: rect.bottom,
        };
        unsafe {
            DrawTextW(
                hdc,
                wide.as_ptr(),
                wide.len() as i32,
                &mut text_rect,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            SelectObject(hdc, old_font);
        }
    }

    unsafe { EndPaint(hwnd, &paint) };
}

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}
