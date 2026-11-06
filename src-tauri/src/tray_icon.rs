use tauri::image::Image;

pub(crate) fn size_for_scale(scale: f64) -> u32 {
    if scale >= 2.0 {
        32
    } else if scale >= 1.5 {
        24
    } else if scale >= 1.25 {
        20
    } else {
        16
    }
}

/// Whether Windows draws the notification area on a light background.
///
/// The taskbar follows the Windows theme (`SystemUsesLightTheme`), which a user
/// can set independently of the app theme Tauri reports, so read the setting
/// instead of the window theme.
#[cfg(windows)]
pub(crate) fn system_uses_light_taskbar() -> bool {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let value: Vec<u16> = "SystemUsesLightTheme\0".encode_utf16().collect();
    let mut light: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated, and `light` is a `u32` buffer of
    // the reported size, matching the `RRF_RT_REG_DWORD` type requested.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut light as *mut u32 as *mut core::ffi::c_void,
            &mut size,
        )
    };
    status == ERROR_SUCCESS && light != 0
}

#[cfg(not(windows))]
pub(crate) fn system_uses_light_taskbar() -> bool {
    false
}

pub(crate) fn image(size: u32, recording: bool, light: bool) -> Image<'static> {
    let rgba: &[u8] = match (size, recording, light) {
        (16, false, false) => include_bytes!("../icons/tray-16.rgba"),
        (16, true, false) => include_bytes!("../icons/tray-16-recording.rgba"),
        (20, false, false) => include_bytes!("../icons/tray-20.rgba"),
        (20, true, false) => include_bytes!("../icons/tray-20-recording.rgba"),
        (24, false, false) => include_bytes!("../icons/tray-24.rgba"),
        (24, true, false) => include_bytes!("../icons/tray-24-recording.rgba"),
        (32, false, false) => include_bytes!("../icons/tray-32.rgba"),
        (32, true, false) => include_bytes!("../icons/tray-32-recording.rgba"),
        (16, false, true) => include_bytes!("../icons/tray-16-light.rgba"),
        (16, true, true) => include_bytes!("../icons/tray-16-recording-light.rgba"),
        (20, false, true) => include_bytes!("../icons/tray-20-light.rgba"),
        (20, true, true) => include_bytes!("../icons/tray-20-recording-light.rgba"),
        (24, false, true) => include_bytes!("../icons/tray-24-light.rgba"),
        (24, true, true) => include_bytes!("../icons/tray-24-recording-light.rgba"),
        (32, false, true) => include_bytes!("../icons/tray-32-light.rgba"),
        (32, true, true) => include_bytes!("../icons/tray-32-recording-light.rgba"),
        _ => return image(16, recording, light),
    };
    Image::new_owned(rgba.to_vec(), size, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(channel: u8) -> f64 {
        let value = f64::from(channel) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(rgb: [u8; 3]) -> f64 {
        0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
    }

    fn contrast(rgb: [u8; 3], background: [u8; 3]) -> f64 {
        let (foreground, background) = (luminance(rgb), luminance(background));
        (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05)
    }

    #[test]
    fn tray_marks_cover_windows_scales_and_recording_state() {
        for (scale, size) in [(1.0, 16), (1.25, 20), (1.5, 24), (2.0, 32)] {
            assert_eq!(size_for_scale(scale), size);
            for light in [false, true] {
                let idle = image(size, false, light);
                let active = image(size, true, light);
                assert_eq!(idle.rgba().len(), (size * size * 4) as usize);
                assert_eq!((idle.width(), idle.height()), (size, size));
                assert_ne!(idle.rgba(), active.rgba());
                assert!(idle.rgba().chunks_exact(4).any(|pixel| pixel[3] == 0));
                assert!(idle.rgba().chunks_exact(4).any(|pixel| pixel[3] == 255));
            }
        }
    }

    #[test]
    fn tray_marks_clear_their_contrast_floor() {
        // Windows 11 notification-area backgrounds. Solid pixels must clear the
        // 3:1 non-text floor, which the white caret failed on a light taskbar.
        // Idle only: the recording dot's black ring separates it from the mark
        // instead of carrying contrast, and both states are covered by
        // `tray_marks_cover_windows_scales_and_recording_state`.
        for (light, background, floor) in [(false, [32, 32, 32], 4.5), (true, [243, 243, 243], 3.0)]
        {
            for size in [16, 20, 24, 32] {
                let icon = image(size, false, light);
                let mut solid = 0;
                for pixel in icon.rgba().chunks_exact(4) {
                    // Edges are antialiased toward transparent black, so only the
                    // solid core carries the palette the user actually sees.
                    if pixel[3] != 255 {
                        continue;
                    }
                    solid += 1;
                    let ratio = contrast([pixel[0], pixel[1], pixel[2]], background);
                    assert!(
                        ratio >= floor,
                        "{size}px light={light}: {pixel:?} is {ratio:.2}:1 against {background:?}"
                    );
                }
                assert!(solid > 0, "{size}px light={light} has no solid pixels");
            }
        }
    }

    #[test]
    fn light_taskbar_marks_swap_the_white_caret_for_ink() {
        let dark = image(32, false, false);
        let light = image(32, false, true);
        assert_ne!(dark.rgba(), light.rgba());
        assert!(
            !light
                .rgba()
                .chunks_exact(4)
                .any(|pixel| pixel[3] == 255 && pixel[..3].iter().all(|channel| *channel > 200)),
            "light taskbar mark still contains white"
        );
        assert!(
            !dark
                .rgba()
                .chunks_exact(4)
                .any(|pixel| pixel[3] == 255 && pixel[..3].iter().all(|channel| *channel < 60)),
            "dark taskbar mark contains ink"
        );
    }

    #[test]
    fn light_taskbar_marks_keep_the_dark_geometry() {
        // The light set is a recolor, so it cannot move the mark or the dot.
        for size in [16, 20, 24, 32] {
            for recording in [false, true] {
                let dark = image(size, recording, false);
                let light = image(size, recording, true);
                for (dark_pixel, light_pixel) in dark
                    .rgba()
                    .chunks_exact(4)
                    .zip(light.rgba().chunks_exact(4))
                {
                    assert_eq!(
                        dark_pixel[3], light_pixel[3],
                        "{size}px recording={recording} moved a pixel"
                    );
                }
            }
        }
    }
}
