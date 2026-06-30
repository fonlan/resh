use crate::config::types::WindowState;
use tauri::WebviewWindow;

const MIN_VISIBLE_LOGICAL_PIXELS: f64 = 64.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn has_visible_area_on(self, other: Self, minimum: f64) -> bool {
        let overlap_width = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let overlap_height =
            (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);

        overlap_width >= minimum && overlap_height >= minimum
    }
}

fn has_valid_geometry(state: &WindowState) -> bool {
    state.width.is_finite()
        && state.height.is_finite()
        && state.width > 0.0
        && state.height > 0.0
        && state.x.is_finite()
        && state.y.is_finite()
}

/// Restores the persisted main-window geometry while ensuring that at least a
/// usable corner remains on a currently connected display.
pub fn restore_window(window: &WebviewWindow, state: &WindowState) {
    if !has_valid_geometry(state) {
        let _ = window.center();
        if state.is_maximized {
            let _ = window.maximize();
        }
        return;
    }

    let size = tauri::LogicalSize::new(state.width, state.height);
    let _ = window.set_size(tauri::Size::Logical(size));

    let saved_rect = Rect {
        x: state.x,
        y: state.y,
        width: state.width,
        height: state.height,
    };

    let position_is_visible = window.available_monitors().map_or(true, |monitors| {
        monitors.is_empty()
            || monitors.iter().any(|monitor| {
                let work_area = monitor.work_area();
                let scale_factor = monitor.scale_factor();
                let position = work_area.position.to_logical::<f64>(scale_factor);
                let size = work_area.size.to_logical::<f64>(scale_factor);
                saved_rect.has_visible_area_on(
                    Rect {
                        x: position.x,
                        y: position.y,
                        width: size.width,
                        height: size.height,
                    },
                    MIN_VISIBLE_LOGICAL_PIXELS,
                )
            })
    });

    if position_is_visible {
        let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
            state.x, state.y,
        )));
    } else {
        let _ = window.center();
    }

    if state.is_maximized {
        let _ = window.maximize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_window_with_a_usable_visible_corner() {
        let window = Rect {
            x: 950.0,
            y: 700.0,
            width: 800.0,
            height: 600.0,
        };
        let monitor = Rect {
            x: 0.0,
            y: 0.0,
            width: 1024.0,
            height: 768.0,
        };

        assert!(window.has_visible_area_on(monitor, 64.0));
    }

    #[test]
    fn rejects_window_left_on_a_disconnected_display() {
        let window = Rect {
            x: 2000.0,
            y: 100.0,
            width: 800.0,
            height: 600.0,
        };
        let monitor = Rect {
            x: 0.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };

        assert!(!window.has_visible_area_on(monitor, 64.0));
    }

    #[test]
    fn supports_monitors_left_of_the_primary_display() {
        let window = Rect {
            x: -1800.0,
            y: 120.0,
            width: 900.0,
            height: 600.0,
        };
        let monitor = Rect {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };

        assert!(window.has_visible_area_on(monitor, 64.0));
    }
}
