pub const TOOLBAR_WIDTH: f64 = 548.0;
pub const TOOLBAR_HEIGHT: f64 = 106.0;
pub const RESULT_WIDTH: f64 = 548.0;
pub const RESULT_HEIGHT: f64 = 356.0;
pub const CURSOR_GAP_LOGICAL: f64 = 12.0;
pub const EDGE_MARGIN_LOGICAL: f64 = 8.0;
pub const VERTICAL_DIRECTION_THRESHOLD_LOGICAL: f64 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionGesture {
    pub start: Anchor,
    pub end: Anchor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementSide {
    Above,
    Below,
}

pub fn selection_window_size(expanded: bool) -> (f64, f64) {
    if expanded {
        (RESULT_WIDTH, RESULT_HEIGHT)
    } else {
        (TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    }
}

pub fn midpoint(first: i32, second: i32) -> i32 {
    ((first as i64 + second as i64).div_euclid(2)) as i32
}

pub fn compute_selection_window_position(
    gesture: SelectionGesture,
    area: WorkArea,
    window_width: i32,
    window_height: i32,
    scale: f64,
) -> (i32, i32, PlacementSide) {
    let gap = (CURSOR_GAP_LOGICAL * scale).round() as i32;
    let edge_margin = (EDGE_MARGIN_LOGICAL * scale).round() as i32;
    let direction_threshold = (VERTICAL_DIRECTION_THRESHOLD_LOGICAL * scale).round() as i32;

    // Keep the popup close to the release cursor, but pull its center one step
    // toward the selection midpoint so long drags do not leave it hanging off
    // the far edge of the selected text.
    let selection_mid_x = midpoint(gesture.start.x, gesture.end.x);
    let focus_x = midpoint(selection_mid_x, gesture.end.x);
    let desired_x = focus_x - window_width / 2;

    let below_y = gesture.end.y + gap;
    let above_y = gesture.end.y - window_height - gap;
    let margin_x = edge_margin.min((area.width - window_width).max(0) / 2);
    let margin_y = edge_margin.min((area.height - window_height).max(0) / 2);
    let top_limit = area.y + margin_y;
    let bottom_limit = area.y + area.height - margin_y;
    let fits_above = above_y >= top_limit;
    let fits_below = below_y + window_height <= bottom_limit;
    let prefers_above = gesture.end.y < gesture.start.y - direction_threshold;

    let side = if prefers_above {
        if fits_above || !fits_below {
            PlacementSide::Above
        } else {
            PlacementSide::Below
        }
    } else if fits_below || !fits_above {
        PlacementSide::Below
    } else {
        PlacementSide::Above
    };
    let desired_y = match side {
        PlacementSide::Above => above_y,
        PlacementSide::Below => below_y,
    };
    let (x, y) = clamp_window_top_left(
        desired_x,
        desired_y,
        area.x + margin_x,
        area.y + margin_y,
        area.width - margin_x * 2,
        area.height - margin_y * 2,
        window_width,
        window_height,
    );
    (x, y, side)
}

#[cfg(target_os = "windows")]
pub fn monitor_work_area(anchor: Anchor) -> Option<WorkArea> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    // SAFETY: MonitorFromPoint takes the POINT by value and a flag, and dereferences no caller pointers.
    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: anchor.x,
                y: anchor.y,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        if monitor.is_null() {
            return None;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        Some(WorkArea {
            x: info.rcWork.left,
            y: info.rcWork.top,
            width: info.rcWork.right - info.rcWork.left,
            height: info.rcWork.bottom - info.rcWork.top,
        })
    }
}

#[cfg(not(target_os = "windows"))]
pub fn monitor_work_area(_anchor: Anchor) -> Option<WorkArea> {
    None
}

#[allow(clippy::too_many_arguments)]
pub fn clamp_window_top_left(
    desired_x: i32,
    desired_y: i32,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: i32,
    monitor_height: i32,
    window_width: i32,
    window_height: i32,
) -> (i32, i32) {
    let max_x = monitor_x + monitor_width - window_width;
    let max_y = monitor_y + monitor_height - window_height;
    (
        desired_x.clamp(monitor_x, max_x.max(monitor_x)),
        desired_y.clamp(monitor_y, max_y.max(monitor_y)),
    )
}

#[cfg(target_os = "windows")]
pub fn point_inside_centered_rectangle(
    center: Anchor,
    point: Anchor,
    width: i32,
    height: i32,
) -> bool {
    coordinate_inside_span(center.x, point.x, width)
        && coordinate_inside_span(center.y, point.y, height)
}

#[cfg(target_os = "windows")]
pub fn coordinate_inside_span(center: i32, value: i32, size: i32) -> bool {
    let size = i64::from(size.max(1));
    let start = i64::from(center) - size / 2;
    let value = i64::from(value);
    value >= start && value < start + size
}
