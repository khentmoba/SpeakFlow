#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::services::screen_capture_service::{self, CapturedScreen};
use crate::state::AppState;

pub mod geometry;
#[cfg(test)]
mod tests;

pub use geometry::*;

const OVERLAY_LABEL: &str = "selection-toolbar";
const SELECTION_SETTLE_MS: u64 = 110;
const MIN_DRAG_DISTANCE_PX: i32 = 4;
const DUPLICATE_WINDOW_MS: u64 = 700;

#[derive(Debug, Clone)]
pub(super) struct SelectionScreenshotContext {
    pub(super) version: u64,
    pub(super) images: Vec<CapturedScreen>,
}

static CURRENT_SELECTION: OnceLock<parking_lot::Mutex<Option<SelectionDetectedPayload>>> =
    OnceLock::new();
static CURRENT_SELECTION_SCREENSHOTS: OnceLock<
    parking_lot::Mutex<Option<SelectionScreenshotContext>>,
> = OnceLock::new();
static SELECTION_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionDetectedPayload {
    pub version: u64,
    pub text: String,
    pub character_count: usize,
    #[serde(skip)]
    source_window: Option<isize>,
}

pub fn current_selection() -> Option<SelectionDetectedPayload> {
    CURRENT_SELECTION
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock()
        .clone()
}

pub fn current_selection_matches(version: u64, selected_text: &str) -> bool {
    let selection = CURRENT_SELECTION
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock();
    selection.as_ref().is_some_and(|selection| {
        selection_context_matches(selection, version, selected_text, foreground_window_id())
    })
}

pub(super) fn selection_context_matches(
    selection: &SelectionDetectedPayload,
    version: u64,
    selected_text: &str,
    foreground_window: Option<isize>,
) -> bool {
    selection.version == version
        && selection.text == selected_text
        && selection.source_window == foreground_window
}

#[cfg(target_os = "windows")]
pub(crate) fn foreground_window_id() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // SAFETY: GetForegroundWindow takes no arguments and returns a window handle value or null.
    let window = unsafe { GetForegroundWindow() };
    (!window.is_null()).then_some(window as isize)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn foreground_window_id() -> Option<isize> {
    None
}

pub fn current_selection_screenshots(selected_text: &str) -> Vec<CapturedScreen> {
    let Some(selection) = current_selection() else {
        return Vec::new();
    };
    let screenshots = CURRENT_SELECTION_SCREENSHOTS
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock();
    matching_selection_screenshots(&selection, screenshots.as_ref(), selected_text)
}

pub(super) fn matching_selection_screenshots(
    selection: &SelectionDetectedPayload,
    context: Option<&SelectionScreenshotContext>,
    selected_text: &str,
) -> Vec<CapturedScreen> {
    if selection.text.trim() != selected_text.trim() {
        return Vec::new();
    }
    context
        .filter(|context| context.version == selection.version)
        .map(|context| context.images.clone())
        .unwrap_or_default()
}

fn clear_selection_screenshots() {
    CURRENT_SELECTION_SCREENSHOTS
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock()
        .take();
}

fn tauri_error(action: &str, error: impl std::fmt::Display) -> String {
    format!("{action}: {error}")
}

pub fn create_selection_window(app_handle: &tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app_handle.get_webview_window(OVERLAY_LABEL) {
        apply_no_activate_style(&window);
        return Ok(());
    }

    let window = tauri::WebviewWindowBuilder::new(
        app_handle,
        OVERLAY_LABEL,
        tauri::WebviewUrl::App("/?window=selection".into()),
    )
    .title("Selection Assistant")
    .inner_size(TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    .transparent(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .resizable(false)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|error| tauri_error("Failed to create selection assistant window", error))?;

    apply_no_activate_style(&window);
    Ok(())
}

pub fn hide_selection_window(app_handle: &tauri::AppHandle) -> Result<(), String> {
    clear_selection_screenshots();
    let Some(window) = app_handle.get_webview_window(OVERLAY_LABEL) else {
        return Ok(());
    };

    let tauri_error = window.hide().err().map(|error| error.to_string());
    if let Some(error) = tauri_error.as_deref() {
        log::warn!(
            "Tauri failed to hide the selection assistant window; will try Win32 SW_HIDE: {error}"
        );
    }

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindowVisible, ShowWindow, SW_HIDE};

        let hwnd = window
            .hwnd()
            .map_err(|error| format!("Failed to get selection assistant HWND: {error}"))?;
        // SAFETY: hwnd is the live window handle Tauri just returned; ShowWindow takes only that handle and a flag.
        unsafe {
            ShowWindow(hwnd.0, SW_HIDE);
        }
        // SAFETY: hwnd is the live window handle Tauri just returned; IsWindowVisible takes only that handle.
        if unsafe { IsWindowVisible(hwnd.0) } != 0 {
            let detail = tauri_error
                .map(|error| format!("Tauri={error}; window remains visible after Win32 SW_HIDE"))
                .unwrap_or_else(|| "Window remains visible after Win32 SW_HIDE".to_string());
            log::error!("{detail}");
            return Err(detail);
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        tauri_error
            .map(|error| {
                Err(format!(
                    "Failed to hide selection assistant window: {error}"
                ))
            })
            .unwrap_or(Ok(()))
    }
}

pub fn start_selection_window_drag(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "Selection assistant window does not exist".to_string())?;

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetCursorPos, SendMessageW, HTCAPTION, WM_NCLBUTTONDOWN,
        };

        let hwnd = window
            .hwnd()
            .map_err(|error| format!("Failed to get selection assistant HWND: {error}"))?;
        let mut cursor = POINT { x: 0, y: 0 };
        // SAFETY: cursor is a valid writable POINT, and the window handle is the live handle Tauri returned above.
        unsafe {
            if GetCursorPos(&mut cursor) == 0 {
                return window
                    .start_dragging()
                    .map_err(|error| tauri_error("Failed to start selection window drag", error));
            }
            ReleaseCapture();
            // WM_NCLBUTTONDOWN expects screen coordinates packed into LPARAM.
            // Passing 0 makes Tao treat the drag as originating at (0, 0),
            // which is unreliable on multi-monitor and scaled desktops.
            let cursor_lparam =
                (((cursor.y as u16 as u32) << 16) | cursor.x as u16 as u32) as isize;
            SendMessageW(hwnd.0, WM_NCLBUTTONDOWN, HTCAPTION as usize, cursor_lparam);
            Ok(())
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        window
            .start_dragging()
            .map_err(|error| tauri_error("Failed to start selection window drag", error))
    }
}

pub fn set_selection_window_expanded(
    app_handle: &tauri::AppHandle,
    expanded: bool,
) -> Result<(), String> {
    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "Selection assistant window does not exist".to_string())?;
    let current_position = window
        .outer_position()
        .map_err(|error| tauri_error("Failed to read selection assistant position", error))?;
    let (logical_width, logical_height) = selection_window_size(expanded);
    let (x, y) = if let Some(monitor) = window
        .current_monitor()
        .map_err(|error| tauri_error("Failed to read selection assistant monitor", error))?
    {
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let scale = monitor.scale_factor();
        clamp_window_top_left(
            current_position.x,
            current_position.y,
            monitor_position.x,
            monitor_position.y,
            monitor_size.width as i32,
            monitor_size.height as i32,
            (logical_width * scale).round() as i32,
            (logical_height * scale).round() as i32,
        )
    } else {
        (current_position.x, current_position.y)
    };

    if expanded {
        window
            .set_min_size(Some(tauri::Size::Logical(tauri::LogicalSize::new(
                420.0, 260.0,
            ))))
            .map_err(|error| {
                tauri_error("Failed to set selection assistant minimum size", error)
            })?;
    } else {
        window.set_min_size(None::<tauri::Size>).map_err(|error| {
            tauri_error("Failed to clear selection assistant minimum size", error)
        })?;
    }
    window
        .set_resizable(expanded)
        .map_err(|error| tauri_error("Failed to toggle selection assistant scaling", error))?;

    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            logical_width,
            logical_height,
        )))
        .map_err(|error| tauri_error("Failed to resize selection assistant window", error))?;
    window
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x, y,
        )))
        .map_err(|error| {
            tauri_error(
                "Failed to preserve selection assistant window position",
                error,
            )
        })?;
    show_without_activation(&window);
    Ok(())
}

fn show_selection(
    app_handle: &tauri::AppHandle,
    gesture: SelectionGesture,
    text: String,
    screenshots: Vec<CapturedScreen>,
) -> Result<(), String> {
    let source_window = foreground_window_id();
    create_selection_window(app_handle)?;
    let window = app_handle
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "Selection assistant window is missing after creation".to_string())?;
    position_window(app_handle, &window, gesture, false)?;
    let version = SELECTION_VERSION.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1;
    let selection = SelectionDetectedPayload {
        version,
        character_count: text.chars().count(),
        text,
        source_window,
    };
    *CURRENT_SELECTION
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock() = Some(selection.clone());
    *CURRENT_SELECTION_SCREENSHOTS
        .get_or_init(|| parking_lot::Mutex::new(None))
        .lock() = (!screenshots.is_empty()).then_some(SelectionScreenshotContext {
        version,
        images: screenshots,
    });
    show_without_activation(&window);
    window
        .emit_to(OVERLAY_LABEL, "selection-detected", selection)
        .map_err(|error| tauri_error("Failed to send selection state", error))?;
    Ok(())
}

fn position_window(
    app_handle: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    gesture: SelectionGesture,
    expanded: bool,
) -> Result<(), String> {
    let (logical_width, logical_height) = selection_window_size(expanded);
    let monitors = app_handle
        .available_monitors()
        .map_err(|error| tauri_error("Failed to read monitor layout", error))?;
    let monitor = monitors
        .iter()
        .find(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            gesture.end.x >= position.x
                && gesture.end.x < position.x + size.width as i32
                && gesture.end.y >= position.y
                && gesture.end.y < position.y + size.height as i32
        })
        .or_else(|| monitors.first());

    let (x, y) = if let Some(monitor) = monitor {
        let position = monitor.position();
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let width = (logical_width * scale).round() as i32;
        let height = (logical_height * scale).round() as i32;
        let fallback_area = WorkArea {
            x: position.x,
            y: position.y,
            width: size.width as i32,
            height: size.height as i32,
        };
        let area = monitor_work_area(gesture.end).unwrap_or(fallback_area);
        let (x, y, _) = compute_selection_window_position(gesture, area, width, height, scale);
        (x, y)
    } else {
        (
            gesture.end.x - (logical_width / 2.0).round() as i32,
            gesture.end.y + CURSOR_GAP_LOGICAL.round() as i32,
        )
    };

    window
        .set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            logical_width,
            logical_height,
        )))
        .map_err(|error| tauri_error("Failed to resize selection assistant window", error))?;
    window
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x, y,
        )))
        .map_err(|error| tauri_error("Failed to position selection assistant window", error))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn apply_no_activate_style(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    if let Ok(hwnd) = window.hwnd() {
        // SAFETY: hwnd is the live window handle Tauri just returned; the style calls take only that handle and integer flags.
        unsafe {
            let style = GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd.0,
                GWL_EXSTYLE,
                style | WS_EX_NOACTIVATE as isize | WS_EX_TOOLWINDOW as isize,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_no_activate_style(_window: &tauri::WebviewWindow) {}

#[cfg(target_os = "windows")]
fn show_without_activation(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SW_SHOWNOACTIVATE,
    };
    if let Ok(hwnd) = window.hwnd() {
        // SAFETY: hwnd is the live window handle Tauri just returned; the calls take only that handle and integer flags.
        unsafe {
            ShowWindow(hwnd.0, SW_SHOWNOACTIVATE);
            SetWindowPos(
                hwnd.0,
                -1isize as _,
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn show_without_activation(window: &tauri::WebviewWindow) {
    let _ = window.show();
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy)]
pub(super) enum HookEvent {
    LeftDown(MouseSample),
    LeftUp(MouseSample, Option<isize>),
    Dismiss,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy)]
pub(super) struct MouseSample {
    pub(super) point: Anchor,
    pub(super) scroll_generation: u64,
    pub(super) interruption_generation: u64,
    pub(super) time_ms: u32,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Default)]
pub(super) struct SelectionInputActivity {
    pub(super) scroll_generation: AtomicU64,
    pub(super) interruption_generation: AtomicU64,
}

#[cfg(target_os = "windows")]
impl SelectionInputActivity {
    pub(super) fn record_scroll(&self) {
        self.scroll_generation.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn interrupt_selection(&self) {
        self.interruption_generation.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn sample(&self, point: Anchor, time_ms: u32) -> MouseSample {
        MouseSample {
            point,
            scroll_generation: self.scroll_generation.load(Ordering::Relaxed),
            interruption_generation: self.interruption_generation.load(Ordering::Relaxed),
            time_ms,
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy)]
pub(super) struct DoubleClickLimits {
    pub(super) max_delay_ms: u32,
    pub(super) rectangle_width: i32,
    pub(super) rectangle_height: i32,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy)]
pub(super) struct ClickCandidate {
    pub(super) press: MouseSample,
    pub(super) source_window: isize,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Default)]
pub(super) struct SelectionGestureTracker {
    pub(super) pending: Option<MouseSample>,
    pub(super) previous_click: Option<ClickCandidate>,
}

#[cfg(target_os = "windows")]
impl SelectionGestureTracker {
    pub(super) fn begin(&mut self, sample: MouseSample) {
        if self.pending.replace(sample).is_some() {
            self.previous_click = None;
        }
    }

    pub(super) fn clear(&mut self) {
        self.pending = None;
        self.previous_click = None;
    }

    pub(super) fn finish(
        &mut self,
        sample: MouseSample,
        source_window: Option<isize>,
        double_click_limits: DoubleClickLimits,
    ) -> Option<SelectionGesture> {
        let Some(start) = self.pending.take() else {
            self.previous_click = None;
            return None;
        };
        let moved = sample.point.x.abs_diff(start.point.x) >= MIN_DRAG_DISTANCE_PX as u32
            || sample.point.y.abs_diff(start.point.y) >= MIN_DRAG_DISTANCE_PX as u32;
        let scrolled = sample.scroll_generation != start.scroll_generation;
        let interrupted = sample.interruption_generation != start.interruption_generation;
        let gesture = SelectionGesture {
            start: start.point,
            end: sample.point,
        };
        if interrupted {
            self.previous_click = None;
            return None;
        }
        if moved || scrolled {
            self.previous_click = None;
            return Some(gesture);
        }

        let Some(source_window) = source_window else {
            self.previous_click = None;
            return None;
        };
        let current_click = ClickCandidate {
            press: start,
            source_window,
        };
        let is_double_click = self.previous_click.take().is_some_and(|previous_click| {
            clicks_form_double_click(previous_click, current_click, double_click_limits)
        });
        if is_double_click {
            self.previous_click = Some(current_click);
            Some(gesture)
        } else {
            self.previous_click = Some(current_click);
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn clicks_form_double_click(
    first: ClickCandidate,
    second: ClickCandidate,
    limits: DoubleClickLimits,
) -> bool {
    first.source_window == second.source_window
        && first.press.scroll_generation == second.press.scroll_generation
        && first.press.interruption_generation == second.press.interruption_generation
        && second.press.time_ms.wrapping_sub(first.press.time_ms) <= limits.max_delay_ms
        && point_inside_centered_rectangle(
            first.press.point,
            second.press.point,
            limits.rectangle_width,
            limits.rectangle_height,
        )
}

#[cfg(target_os = "windows")]
fn windows_double_click_limits() -> DoubleClickLimits {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXDOUBLECLK, SM_CYDOUBLECLK,
    };

    // SAFETY: GetDoubleClickTime takes no arguments and cannot fail.
    unsafe {
        DoubleClickLimits {
            max_delay_ms: GetDoubleClickTime(),
            rectangle_width: GetSystemMetrics(SM_CXDOUBLECLK).max(1),
            rectangle_height: GetSystemMetrics(SM_CYDOUBLECLK).max(1),
        }
    }
}

#[cfg(target_os = "windows")]
static HOOK_ENABLED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static INPUT_ACTIVITY: SelectionInputActivity = SelectionInputActivity {
    scroll_generation: AtomicU64::new(0),
    interruption_generation: AtomicU64::new(0),
};
#[cfg(target_os = "windows")]
static HOOK_SENDER: OnceLock<parking_lot::Mutex<Option<std::sync::mpsc::SyncSender<HookEvent>>>> =
    OnceLock::new();

#[cfg(target_os = "windows")]
fn hook_sender() -> &'static parking_lot::Mutex<Option<std::sync::mpsc::SyncSender<HookEvent>>> {
    HOOK_SENDER.get_or_init(|| parking_lot::Mutex::new(None))
}

#[cfg(target_os = "windows")]
pub fn start_selection_listener(app_handle: tauri::AppHandle) -> Result<(), String> {
    HOOK_ENABLED.store(true, Ordering::Release);
    let mut current_sender = hook_sender().lock();
    if current_sender.is_some() {
        return Ok(());
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(64);
    *current_sender = Some(sender);
    drop(current_sender);

    if let Err(error) = std::thread::Builder::new()
        .name("selection-worker".to_string())
        .spawn(move || run_selection_worker(app_handle, receiver))
    {
        *hook_sender().lock() = None;
        HOOK_ENABLED.store(false, Ordering::Release);
        return Err(format!("Failed to start selection worker thread: {error}"));
    }

    if let Err(error) = std::thread::Builder::new()
        .name("selection-hook".to_string())
        .spawn(run_hook_message_loop)
    {
        *hook_sender().lock() = None;
        HOOK_ENABLED.store(false, Ordering::Release);
        return Err(format!("Failed to start selection input listener: {error}"));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn start_selection_listener(_app_handle: tauri::AppHandle) -> Result<(), String> {
    Ok(())
}

pub fn set_selection_listener_enabled(enabled: bool) {
    #[cfg(target_os = "windows")]
    HOOK_ENABLED.store(enabled, Ordering::Release);

    #[cfg(not(target_os = "windows"))]
    let _ = enabled;
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn mouse_hook(
    code: i32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallNextHookEx, MSLLHOOKSTRUCT};
    if code >= 0 {
        let hook_data = &*(lparam as *const MSLLHOOKSTRUCT);
        let point = hook_data.pt;
        let anchor = Anchor {
            x: point.x,
            y: point.y,
        };
        let message = wparam as u32;
        let source_window = (message == windows_sys::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP)
            .then(foreground_window_id)
            .flatten();
        let event = capture_mouse_hook_event(
            message,
            anchor,
            hook_data.time,
            source_window,
            &INPUT_ACTIVITY,
        );
        let sender = if HOOK_ENABLED.load(Ordering::Acquire) {
            hook_sender().lock().clone()
        } else {
            None
        };
        if let (Some(sender), Some(event)) = (sender, event) {
            let _ = sender.try_send(event);
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn is_mouse_wheel_message(message: u32) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_MOUSEHWHEEL, WM_MOUSEWHEEL};

    matches!(message, WM_MOUSEWHEEL | WM_MOUSEHWHEEL)
}

#[cfg(target_os = "windows")]
fn is_other_mouse_button_down(message: u32) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
    };

    matches!(message, WM_MBUTTONDOWN | WM_RBUTTONDOWN | WM_XBUTTONDOWN)
}

#[cfg(target_os = "windows")]
pub(super) fn capture_mouse_hook_event(
    message: u32,
    point: Anchor,
    time_ms: u32,
    source_window: Option<isize>,
    input_activity: &SelectionInputActivity,
) -> Option<HookEvent> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_LBUTTONDOWN, WM_LBUTTONUP};

    if is_mouse_wheel_message(message) {
        input_activity.record_scroll();
        return None;
    }
    if is_other_mouse_button_down(message) {
        input_activity.interrupt_selection();
        return None;
    }

    let sample = input_activity.sample(point, time_ms);
    match message {
        WM_LBUTTONDOWN => Some(HookEvent::LeftDown(sample)),
        WM_LBUTTONUP => Some(HookEvent::LeftUp(sample, source_window)),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn keyboard_hook(
    code: i32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, WM_KEYDOWN, WM_SYSKEYDOWN,
    };
    if code >= 0 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let event = &*(lparam as *const KBDLLHOOKSTRUCT);
        if event.flags & LLKHF_INJECTED == 0 {
            INPUT_ACTIVITY.interrupt_selection();
            let sender = if HOOK_ENABLED.load(Ordering::Acquire) {
                hook_sender().lock().clone()
            } else {
                None
            };
            if let Some(sender) = sender {
                let _ = sender.try_send(HookEvent::Dismiss);
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn run_hook_message_loop() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        WH_KEYBOARD_LL, WH_MOUSE_LL,
    };
    // SAFETY: The hook procedures are static extern "system" callbacks with the signatures these hooks require; each handle is unhooked on the failure path and after the loop exits.
    unsafe {
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), std::ptr::null_mut(), 0);
        let keyboard =
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), std::ptr::null_mut(), 0);
        if mouse.is_null() || keyboard.is_null() {
            log::error!(
                "Failed to install the global input listener for selection: {}",
                std::io::Error::last_os_error()
            );
            if !mouse.is_null() {
                UnhookWindowsHookEx(mouse);
            }
            if !keyboard.is_null() {
                UnhookWindowsHookEx(keyboard);
            }
            *hook_sender().lock() = None;
            HOOK_ENABLED.store(false, Ordering::Release);
            return;
        }
        let mut message = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(mouse);
        UnhookWindowsHookEx(keyboard);
        *hook_sender().lock() = None;
        HOOK_ENABLED.store(false, Ordering::Release);
    }
}

#[cfg(target_os = "windows")]
fn run_selection_worker(
    app_handle: tauri::AppHandle,
    receiver: std::sync::mpsc::Receiver<HookEvent>,
) {
    let mut gesture_tracker = SelectionGestureTracker::default();
    let double_click_limits = windows_double_click_limits();
    let mut last_value = String::new();
    let mut last_seen = Instant::now() - Duration::from_secs(2);

    while let Ok(event) = receiver.recv() {
        match event {
            HookEvent::Dismiss => {
                gesture_tracker.clear();
                if let Err(error) = hide_selection_window(&app_handle) {
                    log::warn!("Failed to hide the selection assistant on an external keyboard event: {error}");
                }
                cancel_active_request(&app_handle);
            }
            HookEvent::LeftDown(sample) => {
                let point = sample.point;
                if point_inside_overlay(&app_handle, point) {
                    gesture_tracker.clear();
                } else {
                    if let Err(error) = hide_selection_window(&app_handle) {
                        log::warn!("Failed to hide the selection assistant on an external mouse event: {error}");
                    }
                    cancel_active_request(&app_handle);
                    gesture_tracker.begin(sample);
                }
            }
            HookEvent::LeftUp(sample, source_window) => {
                let point = sample.point;
                if point_inside_overlay(&app_handle, point) {
                    gesture_tracker.clear();
                    continue;
                }
                let Some(gesture) =
                    gesture_tracker.finish(sample, source_window, double_click_limits)
                else {
                    continue;
                };

                let config = app_handle
                    .state::<AppState>()
                    .with_profile(|profile| profile.selection_assistant.clone());
                if !config.enabled {
                    continue;
                }
                let Some(source_app) = foreground_app_source(&config.excluded_apps) else {
                    continue;
                };
                let screenshot_task = if config.auto_screenshot {
                    match std::thread::Builder::new()
                        .name("selection-screenshot".to_string())
                        .spawn(move || {
                            screen_capture_service::capture_screen_context_at_point(
                                point.x, point.y,
                            )
                        }) {
                        Ok(task) => Some(task),
                        Err(error) => {
                            log::warn!("Failed to start automatic selection screenshot; falling back to text only: {error}");
                            None
                        }
                    }
                } else {
                    None
                };

                std::thread::sleep(Duration::from_millis(SELECTION_SETTLE_MS));
                let Some(text) = crate::commands::clipboard::grab_selected_text_robust(&app_handle)
                else {
                    continue;
                };
                let dedupe_value = format!("{source_app}\0{text}");
                if dedupe_value == last_value
                    && last_seen.elapsed() < Duration::from_millis(DUPLICATE_WINDOW_MS)
                {
                    continue;
                }
                last_value = dedupe_value;
                last_seen = Instant::now();
                let screenshots = match screenshot_task {
                    Some(task) => match task.join() {
                        Ok(Ok(images)) => images,
                        Ok(Err(error)) => {
                            log::warn!("Automatic selection screenshot failed; falling back to text only: {error}");
                            Vec::new()
                        }
                        Err(_) => {
                            log::warn!("Automatic selection screenshot thread failed unexpectedly; falling back to text only");
                            Vec::new()
                        }
                    },
                    None => Vec::new(),
                };
                if let Err(error) = show_selection(&app_handle, gesture, text, screenshots) {
                    log::warn!("Failed to show the selection assistant: {error}");
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn cancel_active_request(app_handle: &tauri::AppHandle) {
    if let Some(task) = app_handle
        .state::<AppState>()
        .ui
        .selection_cancel
        .lock()
        .take()
    {
        let _ = task.cancel.send(());
    }
}

#[cfg(target_os = "windows")]
fn point_inside_overlay(app_handle: &tauri::AppHandle, point: Anchor) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};

    let Some(window) = app_handle.get_webview_window(OVERLAY_LABEL) else {
        return false;
    };
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    // SAFETY: hwnd is the live window handle Tauri just returned; IsWindowVisible takes only that handle.
    if unsafe { IsWindowVisible(hwnd.0) } == 0 {
        return false;
    }
    // SAFETY: RECT is a plain C struct of integers, so the all-zero bit pattern is a valid value.
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    // SAFETY: rect is a valid writable RECT, and hwnd is the live handle Tauri returned.
    let valid = unsafe { GetWindowRect(hwnd.0, &mut rect) != 0 };
    valid
        && point.x >= rect.left
        && point.x < rect.right
        && point.y >= rect.top
        && point.y < rect.bottom
}

#[cfg(target_os = "windows")]
fn foreground_app_source(excluded_apps: &[String]) -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, GWL_STYLE,
        WS_CAPTION,
    };

    // SAFETY: The foreground handle is null-checked before use, and the remaining calls take that handle or valid writable locals.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }

        // Borderless monitor-sized windows are typically games or capture
        // overlays. Maximized normal applications retain WS_CAPTION and remain
        // eligible for selection.
        let mut rect: RECT = std::mem::zeroed();
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetWindowRect(window, &mut rect) != 0
            && !monitor.is_null()
            && GetMonitorInfoW(monitor, &mut info) != 0
            && GetWindowLongW(window, GWL_STYLE) as u32 & WS_CAPTION == 0
            && rect.left <= info.rcMonitor.left
            && rect.top <= info.rcMonitor.top
            && rect.right >= info.rcMonitor.right
            && rect.bottom >= info.rcMonitor.bottom
        {
            return None;
        }

        let mut process_id = 0;
        GetWindowThreadProcessId(window, &mut process_id);
        if process_id == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) != 0;
        CloseHandle(process);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        let name = path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(path.as_str())
            .to_ascii_lowercase();
        let filtered = excluded_apps
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .any(|value| !value.is_empty() && value == name);
        (!filtered).then_some(name)
    }
}
