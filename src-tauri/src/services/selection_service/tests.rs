use super::geometry::{
    clamp_window_top_left, compute_selection_window_position, Anchor, PlacementSide,
    SelectionGesture, WorkArea,
};
use super::{
    matching_selection_screenshots, selection_context_matches, SelectionDetectedPayload,
    SelectionScreenshotContext,
};
use crate::services::screen_capture_service::CapturedScreen;

const PRIMARY_WORK_AREA: WorkArea = WorkArea {
    x: 0,
    y: 0,
    width: 1920,
    height: 1040,
};

#[test]
fn screenshot_context_is_scoped_to_the_exact_selection_version_and_text() {
    let selection = SelectionDetectedPayload {
        version: 7,
        text: " selected text ".to_string(),
        character_count: 13,
        source_window: Some(42),
    };
    let context = SelectionScreenshotContext {
        version: 7,
        images: vec![CapturedScreen {
            mime_type: "image/jpeg".to_string(),
            data_base64: "encoded".to_string(),
        }],
    };

    assert_eq!(
        matching_selection_screenshots(&selection, Some(&context), "selected text").len(),
        1
    );
    assert!(matching_selection_screenshots(&selection, Some(&context), "other").is_empty());
    assert!(matching_selection_screenshots(
        &selection,
        Some(&SelectionScreenshotContext {
            version: 6,
            images: context.images,
        }),
        "selected text",
    )
    .is_empty());
}

#[test]
fn replacement_context_requires_the_exact_version_text_and_source_window() {
    let selection = SelectionDetectedPayload {
        version: 7,
        text: "selected text".to_string(),
        character_count: 13,
        source_window: Some(42),
    };

    assert!(selection_context_matches(
        &selection,
        7,
        "selected text",
        Some(42)
    ));
    assert!(!selection_context_matches(
        &selection,
        8,
        "selected text",
        Some(42)
    ));
    assert!(!selection_context_matches(
        &selection,
        7,
        "other text",
        Some(42)
    ));
    assert!(!selection_context_matches(
        &selection,
        7,
        "selected text",
        Some(99)
    ));
}

#[test]
fn centers_near_the_release_cursor_but_nudges_toward_the_selection() {
    let (x, y, side) = compute_selection_window_position(
        SelectionGesture {
            start: Anchor { x: 400, y: 280 },
            end: Anchor { x: 700, y: 300 },
        },
        PRIMARY_WORK_AREA,
        548,
        106,
        1.0,
    );

    assert_eq!((x, y, side), (351, 312, PlacementSide::Below));
    assert_eq!(x + 548 / 2, 625);
}

#[test]
fn upward_multiline_selection_places_the_popup_above_the_release_cursor() {
    assert_eq!(
        compute_selection_window_position(
            SelectionGesture {
                start: Anchor { x: 700, y: 500 },
                end: Anchor { x: 650, y: 250 },
            },
            PRIMARY_WORK_AREA,
            548,
            106,
            1.0,
        ),
        (388, 132, PlacementSide::Above)
    );
}

#[test]
fn slight_same_line_vertical_jitter_does_not_flip_the_popup() {
    let (_, y, side) = compute_selection_window_position(
        SelectionGesture {
            start: Anchor { x: 400, y: 302 },
            end: Anchor { x: 700, y: 300 },
        },
        PRIMARY_WORK_AREA,
        548,
        106,
        1.0,
    );

    assert_eq!((y, side), (312, PlacementSide::Below));
}

#[test]
fn flips_above_and_keeps_an_edge_margin_near_the_taskbar() {
    assert_eq!(
        compute_selection_window_position(
            SelectionGesture {
                start: Anchor { x: 1800, y: 1000 },
                end: Anchor { x: 1915, y: 1035 },
            },
            PRIMARY_WORK_AREA,
            548,
            106,
            1.0,
        ),
        (1364, 917, PlacementSide::Above)
    );
}

#[test]
fn supports_negative_coordinate_work_areas() {
    assert_eq!(
        compute_selection_window_position(
            SelectionGesture {
                start: Anchor { x: -1240, y: 35 },
                end: Anchor { x: -1200, y: 40 },
            },
            WorkArea {
                x: -1280,
                y: 24,
                width: 1280,
                height: 984,
            },
            548,
            106,
            1.0,
        ),
        (-1272, 52, PlacementSide::Below)
    );
}

#[test]
fn keeps_a_dragged_position_that_still_fits_after_resize() {
    assert_eq!(
        clamp_window_top_left(420, 180, 0, 0, 1920, 1080, 548, 356),
        (420, 180)
    );
}

#[test]
fn moves_only_enough_to_keep_an_expanded_window_on_screen() {
    assert_eq!(
        clamp_window_top_left(1600, 900, 0, 0, 1920, 1080, 548, 356),
        (1372, 724)
    );
}

#[test]
fn supports_monitors_with_negative_desktop_coordinates() {
    assert_eq!(
        clamp_window_top_left(-1800, -20, -1920, 0, 1920, 1080, 548, 356),
        (-1800, 0)
    );
}

#[cfg(target_os = "windows")]
mod gesture_tests {
    use super::super::{
        capture_mouse_hook_event, DoubleClickLimits, HookEvent, MouseSample,
        SelectionGestureTracker, SelectionInputActivity,
    };
    use super::{Anchor, SelectionGesture};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
        WM_RBUTTONDOWN, WM_XBUTTONDOWN,
    };

    const POINT: Anchor = Anchor { x: 640, y: 480 };
    const SOURCE_WINDOW: isize = 42;
    const OTHER_SOURCE_WINDOW: isize = 84;
    const DOUBLE_CLICK_LIMITS: DoubleClickLimits = DoubleClickLimits {
        max_delay_ms: 500,
        rectangle_width: 4,
        rectangle_height: 4,
    };

    fn sample(point: Anchor, scroll_generation: u64, time_ms: u32) -> MouseSample {
        MouseSample {
            point,
            scroll_generation,
            interruption_generation: 0,
            time_ms,
        }
    }

    fn click(
        tracker: &mut SelectionGestureTracker,
        point: Anchor,
        scroll_generation: u64,
        time_ms: u32,
        source_window: Option<isize>,
    ) -> Option<SelectionGesture> {
        tracker.begin(sample(point, scroll_generation, time_ms));
        tracker.finish(
            sample(point, scroll_generation, time_ms.wrapping_add(10)),
            source_window,
            DOUBLE_CLICK_LIMITS,
        )
    }

    #[test]
    fn wheel_assisted_selection_is_eligible_even_when_release_position_is_unchanged() {
        let mut tracker = SelectionGestureTracker::default();
        tracker.begin(sample(POINT, 7, 100));

        let gesture = tracker
            .finish(
                sample(POINT, 8, 150),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS,
            )
            .expect("wheel activity while the button is held indicates selection intent");
        assert_eq!((gesture.start.x, gesture.start.y), (POINT.x, POINT.y));
        assert_eq!((gesture.end.x, gesture.end.y), (POINT.x, POINT.y));
    }

    #[test]
    fn movement_below_the_existing_threshold_without_scroll_remains_ineligible() {
        let mut tracker = SelectionGestureTracker::default();
        tracker.begin(sample(POINT, 7, 100));

        assert!(tracker
            .finish(
                sample(
                    Anchor {
                        x: POINT.x + 2,
                        y: POINT.y + 2
                    },
                    7,
                    150
                ),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS,
            )
            .is_none());
    }

    #[test]
    fn ordinary_drag_at_the_existing_threshold_remains_eligible() {
        let mut tracker = SelectionGestureTracker::default();
        tracker.begin(sample(POINT, 7, 100));

        let gesture = tracker
            .finish(
                sample(
                    Anchor {
                        x: POINT.x + 4,
                        y: POINT.y,
                    },
                    7,
                    150,
                ),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS,
            )
            .expect("drag at the threshold should produce a gesture");
        assert_eq!((gesture.start.x, gesture.start.y), (POINT.x, POINT.y));
        assert_eq!((gesture.end.x, gesture.end.y), (POINT.x + 4, POINT.y));
    }

    #[test]
    fn one_plain_click_remains_ineligible() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());
    }

    #[test]
    fn second_plain_click_in_the_same_window_emits_a_gesture() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());

        let gesture = click(&mut tracker, POINT, 0, 300, Some(SOURCE_WINDOW))
            .expect("second click within limits should emit a gesture");
        assert_eq!((gesture.start.x, gesture.start.y), (POINT.x, POINT.y));
        assert_eq!((gesture.end.x, gesture.end.y), (POINT.x, POINT.y));
    }

    #[test]
    fn third_plain_click_remains_eligible_for_native_triple_click_selection() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 0, 300, Some(SOURCE_WINDOW)).is_some());
        assert!(click(&mut tracker, POINT, 0, 500, Some(SOURCE_WINDOW)).is_some());
    }

    #[test]
    fn double_click_delay_is_inclusive_and_a_late_click_becomes_the_next_candidate() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 0, 600, Some(SOURCE_WINDOW)).is_some());

        let mut late_tracker = SelectionGestureTracker::default();
        assert!(click(&mut late_tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut late_tracker, POINT, 0, 601, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut late_tracker, POINT, 0, 900, Some(SOURCE_WINDOW)).is_some());
    }

    #[test]
    fn double_click_rectangle_uses_its_full_width_and_height() {
        let corners = [
            Anchor {
                x: POINT.x - 2,
                y: POINT.y - 2,
            },
            Anchor {
                x: POINT.x + 1,
                y: POINT.y + 1,
            },
            Anchor {
                x: POINT.x - 2,
                y: POINT.y + 1,
            },
            Anchor {
                x: POINT.x + 1,
                y: POINT.y - 2,
            },
        ];
        for corner in corners {
            let mut tracker = SelectionGestureTracker::default();
            assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());
            assert!(
                click(&mut tracker, corner, 0, 300, Some(SOURCE_WINDOW)).is_some(),
                "corner {corner:?} should remain within the double click rectangle"
            );
        }

        let outside = [
            Anchor {
                x: POINT.x - 3,
                y: POINT.y,
            },
            Anchor {
                x: POINT.x + 2,
                y: POINT.y,
            },
            Anchor {
                x: POINT.x,
                y: POINT.y - 3,
            },
            Anchor {
                x: POINT.x,
                y: POINT.y + 2,
            },
        ];
        for point in outside {
            let mut tracker = SelectionGestureTracker::default();
            assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());
            assert!(
                click(&mut tracker, point, 0, 300, Some(SOURCE_WINDOW)).is_none(),
                "point {point:?} must fall outside the double click rectangle"
            );
        }
    }

    #[test]
    fn clicks_from_different_foreground_windows_do_not_pair() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 0, 300, Some(OTHER_SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 0, 500, Some(SOURCE_WINDOW)).is_none());
    }

    #[test]
    fn scroll_between_clicks_breaks_the_pair() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 1, 300, Some(SOURCE_WINDOW)).is_none());
    }

    #[test]
    fn drag_selection_clears_the_previous_click_candidate() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());

        tracker.begin(sample(POINT, 0, 200));
        let drag_release = Anchor {
            x: POINT.x + 10,
            y: POINT.y,
        };
        assert!(tracker
            .finish(
                sample(drag_release, 0, 250),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS,
            )
            .is_some());

        assert!(click(&mut tracker, POINT, 0, 350, Some(SOURCE_WINDOW)).is_none());
    }

    #[test]
    fn clear_removes_both_pending_and_previous_click_state() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());
        tracker.begin(sample(POINT, 0, 200));
        tracker.clear();

        assert!(tracker
            .finish(
                sample(POINT, 0, 250),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS
            )
            .is_none());
        assert!(click(&mut tracker, POINT, 0, 350, Some(SOURCE_WINDOW)).is_none());
    }

    #[test]
    fn unmatched_button_up_clears_a_stale_click_candidate() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, 100, Some(SOURCE_WINDOW)).is_none());

        assert!(tracker
            .finish(
                sample(POINT, 0, 200),
                Some(SOURCE_WINDOW),
                DOUBLE_CLICK_LIMITS
            )
            .is_none());
        assert!(click(&mut tracker, POINT, 0, 300, Some(SOURCE_WINDOW)).is_none());
    }

    #[test]
    fn double_click_time_handles_u32_wraparound() {
        let mut tracker = SelectionGestureTracker::default();
        assert!(click(&mut tracker, POINT, 0, u32::MAX - 100, Some(SOURCE_WINDOW)).is_none());
        assert!(click(&mut tracker, POINT, 0, 50, Some(SOURCE_WINDOW)).is_some());
    }

    #[test]
    fn scrolling_without_an_active_press_does_not_pollute_the_next_click() {
        let input_activity = SelectionInputActivity::default();
        let mut tracker = SelectionGestureTracker::default();

        assert!(
            capture_mouse_hook_event(WM_MOUSEWHEEL, POINT, 100, None, &input_activity).is_none()
        );
        assert_eq!(
            input_activity
                .scroll_generation
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );

        let Some(HookEvent::LeftDown(down_sample)) =
            capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 200, None, &input_activity)
        else {
            panic!("left-button down must be forwarded");
        };
        assert_eq!(down_sample.scroll_generation, 1);
        tracker.begin(down_sample);

        let Some(HookEvent::LeftUp(up_sample, source_window)) = capture_mouse_hook_event(
            WM_LBUTTONUP,
            POINT,
            250,
            Some(SOURCE_WINDOW),
            &input_activity,
        ) else {
            panic!("left-button up must be forwarded");
        };
        assert_eq!(up_sample.scroll_generation, 1);
        assert!(tracker
            .finish(up_sample, source_window, DOUBLE_CLICK_LIMITS)
            .is_none());
    }

    #[test]
    fn low_level_mouse_hook_records_vertical_and_horizontal_wheel_activity() {
        for wheel_message in [WM_MOUSEWHEEL, WM_MOUSEHWHEEL] {
            let input_activity = SelectionInputActivity::default();
            let mut tracker = SelectionGestureTracker::default();

            let Some(HookEvent::LeftDown(down_sample)) =
                capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 100, None, &input_activity)
            else {
                panic!("left-button down must be forwarded");
            };
            tracker.begin(down_sample);

            assert!(
                capture_mouse_hook_event(wheel_message, POINT, 150, None, &input_activity)
                    .is_none()
            );
            assert_eq!(
                input_activity
                    .scroll_generation
                    .load(std::sync::atomic::Ordering::Relaxed),
                1
            );

            let Some(HookEvent::LeftUp(sample, source_window)) = capture_mouse_hook_event(
                WM_LBUTTONUP,
                POINT,
                200,
                Some(SOURCE_WINDOW),
                &input_activity,
            ) else {
                panic!("left-button up must be forwarded");
            };
            assert_eq!(sample.scroll_generation, 1);
            assert!(tracker
                .finish(sample, source_window, DOUBLE_CLICK_LIMITS)
                .is_some());
        }
    }

    #[test]
    fn other_mouse_buttons_interrupt_a_double_click_sequence() {
        for interrupt_message in [WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN] {
            let input_activity = SelectionInputActivity::default();
            let mut tracker = SelectionGestureTracker::default();

            let Some(HookEvent::LeftDown(first_down)) =
                capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 100, None, &input_activity)
            else {
                panic!("left-button down must be forwarded");
            };
            tracker.begin(first_down);
            let Some(HookEvent::LeftUp(first_up, first_window)) = capture_mouse_hook_event(
                WM_LBUTTONUP,
                POINT,
                110,
                Some(SOURCE_WINDOW),
                &input_activity,
            ) else {
                panic!("left-button up must be forwarded");
            };
            assert!(tracker
                .finish(first_up, first_window, DOUBLE_CLICK_LIMITS)
                .is_none());

            assert!(
                capture_mouse_hook_event(interrupt_message, POINT, 150, None, &input_activity,)
                    .is_none()
            );

            let Some(HookEvent::LeftDown(second_down)) =
                capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 200, None, &input_activity)
            else {
                panic!("left-button down must be forwarded");
            };
            tracker.begin(second_down);
            let Some(HookEvent::LeftUp(second_up, second_window)) = capture_mouse_hook_event(
                WM_LBUTTONUP,
                POINT,
                210,
                Some(SOURCE_WINDOW),
                &input_activity,
            ) else {
                panic!("left-button up must be forwarded");
            };
            assert!(tracker
                .finish(second_up, second_window, DOUBLE_CLICK_LIMITS)
                .is_none());
        }
    }

    #[test]
    fn other_mouse_button_during_a_left_press_is_not_treated_as_scroll_selection() {
        let input_activity = SelectionInputActivity::default();
        let mut tracker = SelectionGestureTracker::default();
        let Some(HookEvent::LeftDown(sample)) =
            capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 100, None, &input_activity)
        else {
            panic!("left-button down must be forwarded");
        };
        tracker.begin(sample);

        assert!(
            capture_mouse_hook_event(WM_RBUTTONDOWN, POINT, 125, None, &input_activity,).is_none()
        );
        let Some(HookEvent::LeftUp(sample, source_window)) = capture_mouse_hook_event(
            WM_LBUTTONUP,
            Anchor {
                x: POINT.x + 4,
                y: POINT.y,
            },
            150,
            Some(SOURCE_WINDOW),
            &input_activity,
        ) else {
            panic!("left-button up must be forwarded");
        };
        assert!(tracker
            .finish(sample, source_window, DOUBLE_CLICK_LIMITS)
            .is_none());
    }

    #[test]
    fn low_level_mouse_hook_preserves_the_message_timestamp() {
        let input_activity = SelectionInputActivity::default();
        input_activity
            .scroll_generation
            .store(9, std::sync::atomic::Ordering::Relaxed);
        let Some(HookEvent::LeftDown(sample)) =
            capture_mouse_hook_event(WM_LBUTTONDOWN, POINT, 1_234, None, &input_activity)
        else {
            panic!("left-button down must be forwarded");
        };

        assert_eq!(sample.time_ms, 1_234);
        assert_eq!(sample.scroll_generation, 9);
    }

    #[test]
    fn low_level_mouse_hook_preserves_the_release_window_snapshot() {
        let input_activity = SelectionInputActivity::default();
        let Some(HookEvent::LeftUp(_, source_window)) = capture_mouse_hook_event(
            WM_LBUTTONUP,
            POINT,
            1_234,
            Some(SOURCE_WINDOW),
            &input_activity,
        ) else {
            panic!("left-button up must be forwarded");
        };

        assert_eq!(source_window, Some(SOURCE_WINDOW));
    }
}
