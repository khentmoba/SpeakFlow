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

/// Accent luminance at which the two palettes trade places.
///
/// 0.3 is where the dark set's white caret falls to the 3:1 non-text floor
/// (`1.05 / 3 - 0.05`), and the two ambers have equal contrast within a
/// hundredth of it, so one split serves both elements of the mark.
const ACCENT_CROSSOVER: f64 = 0.3;

fn linear(channel: u8) -> f64 {
    let value = f64::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Relative luminance of an sRGB colour, per WCAG 2.2.
fn luminance(rgb: [u8; 3]) -> f64 {
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

/// Whether the notification area is light for these Windows settings.
///
/// `accent` is the colour the taskbar is painted in when the user turned on
/// "Show accent color on Start and taskbar". Windows only offers that setting
/// in dark mode, so a light taskbar is never accent-coloured and the theme
/// decides there; a leftover `ColorPrevalence` must not override it. In dark
/// mode the accent replaces the background, and a bright accent needs the light
/// set even though the theme is dark.
fn taskbar_is_light_for(light_theme: bool, accent: Option<[u8; 3]>) -> bool {
    if light_theme {
        return true;
    }
    accent.is_some_and(|rgb| luminance(rgb) > ACCENT_CROSSOVER)
}

#[cfg(windows)]
const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
#[cfg(windows)]
const DWM: &str = r"Software\Microsoft\Windows\DWM";

/// Reads a `REG_DWORD` from `HKEY_CURRENT_USER`, or `None` if it is unset.
#[cfg(windows)]
fn read_dword(subkey: &str, value: &str) -> Option<u32> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    let subkey: Vec<u16> = subkey.encode_utf16().chain([0]).collect();
    let value: Vec<u16> = value.encode_utf16().chain([0]).collect();
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated and outlive the call, and `data`
    // is a `u32` buffer of the size passed, matching `RRF_RT_REG_DWORD`.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut core::ffi::c_void,
            &mut size,
        )
    };
    (status == ERROR_SUCCESS && size == std::mem::size_of::<u32>() as u32).then_some(data)
}

/// The RGB of a `ColorizationColor` value, which Windows stores as ARGB.
#[cfg(windows)]
fn rgb_from_argb(argb: u32) -> [u8; 3] {
    [(argb >> 16) as u8, (argb >> 8) as u8, argb as u8]
}

/// The RGB of an `AccentColor` value, which Windows stores as ABGR.
#[cfg(windows)]
fn rgb_from_abgr(abgr: u32) -> [u8; 3] {
    [abgr as u8, (abgr >> 8) as u8, (abgr >> 16) as u8]
}

/// The colour Windows paints the notification area in, when the user asked for
/// the accent there.
#[cfg(windows)]
fn taskbar_accent() -> Option<[u8; 3]> {
    if read_dword(PERSONALIZE, "ColorPrevalence").unwrap_or(0) == 0 {
        return None;
    }
    // `ColorizationColor` is the taskbar colour. `AccentColor` is the same
    // colour in another byte order, kept as the fallback for a profile that has
    // never written the first.
    read_dword(DWM, "ColorizationColor")
        .map(rgb_from_argb)
        .or_else(|| read_dword(DWM, "AccentColor").map(rgb_from_abgr))
}

/// Whether Windows draws the notification area on a light background.
///
/// The taskbar follows the Windows theme (`SystemUsesLightTheme`), which a user
/// can set independently of the app theme Tauri reports, so read the setting
/// instead of the window theme. "Show accent color on Start and taskbar"
/// overrides the background with the accent colour, so the accent decides then.
#[cfg(windows)]
pub(crate) fn taskbar_is_light() -> bool {
    let light_theme = read_dword(PERSONALIZE, "SystemUsesLightTheme").unwrap_or(0) != 0;
    taskbar_is_light_for(light_theme, taskbar_accent())
}

#[cfg(not(windows))]
pub(crate) fn taskbar_is_light() -> bool {
    taskbar_is_light_for(false, None)
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

    /// Accent colours from the Windows 11 palette: the default blue, the
    /// neutral row, a mid-tone purple, teal, gold and the light grey.
    const ACCENTS: [[u8; 3]; 6] = [
        [0x00, 0x78, 0xD4],
        [0x76, 0x76, 0x76],
        [0x8E, 0x8C, 0xD8],
        [0x00, 0xB2, 0x94],
        [0xFF, 0xB9, 0x00],
        [0xD0, 0xD0, 0xD0],
    ];

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

    #[test]
    fn accent_taskbar_only_overrides_a_dark_theme() {
        // Windows grays out "Show accent color on Start and taskbar" in light
        // mode, so a leftover ColorPrevalence must not turn a light taskbar
        // dark. The accent is only the background when Windows is dark.
        assert!(taskbar_is_light_for(true, Some([0x00, 0x78, 0xD4])));
        assert!(taskbar_is_light_for(true, Some([0xFF, 0xB9, 0x00])));
        // In dark mode a dark accent keeps the dark set, and a bright one needs
        // the light set: the white caret measured 1.7:1 on gold.
        assert!(!taskbar_is_light_for(false, Some([0x00, 0x78, 0xD4])));
        assert!(taskbar_is_light_for(false, Some([0xFF, 0xB9, 0x00])));
        // Without an accent on the taskbar the Windows theme decides.
        assert!(taskbar_is_light_for(true, None));
        assert!(!taskbar_is_light_for(false, None));
    }

    #[cfg(windows)]
    #[test]
    fn accent_decodes_both_registry_encodings() {
        // Read from a Windows 11 profile: the same #0078D4 accent stored as
        // ARGB in ColorizationColor and ABGR in AccentColor.
        assert_eq!(rgb_from_argb(0xC400_78D4), [0x00, 0x78, 0xD4]);
        assert_eq!(rgb_from_abgr(0xFFD4_7800), [0x00, 0x78, 0xD4]);
        assert_eq!(rgb_from_argb(0xFFD4_7800), [0xD4, 0x78, 0x00]);
    }

    #[test]
    fn accent_palettes_keep_the_caret_above_the_floor() {
        // The accent replaces the background, so the caret has to clear 3:1 on
        // the accent itself, not on the theme background it overrides.
        for accent in ACCENTS {
            let light = taskbar_is_light_for(false, Some(accent));
            for size in [16, 20, 24, 32] {
                let icon = image(size, false, light);
                let width = size as usize;
                let mut solid = 0;
                for (index, pixel) in icon.rgba().chunks_exact(4).enumerate() {
                    // The caret is the only element in the right fifth of the
                    // mark, so this picks it without naming a palette colour.
                    if pixel[3] != 255 || index % width < width * 4 / 5 {
                        continue;
                    }
                    solid += 1;
                    let ratio = contrast([pixel[0], pixel[1], pixel[2]], accent);
                    assert!(
                        ratio >= 3.0,
                        "{size}px accent {accent:?}: caret pixel {pixel:?} is {ratio:.2}:1"
                    );
                }
                assert!(solid > 0, "{size}px accent {accent:?} has no solid caret");
            }
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Reads the live Windows personalisation; run manually"]
    fn live_accent_keys_are_readable() {
        // Exercises the registry FFI against the live profile. The values depend
        // on the user's settings, so this is manual, but a typo'd path would
        // silently fall back to the theme. The byte orders are pinned by
        // `accent_decodes_both_registry_encodings`; `ColorizationColor` is the
        // derived taskbar colour and may legitimately differ from `AccentColor`
        // on a custom accent, so the two are not compared here.
        assert!(read_dword(PERSONALIZE, "SystemUsesLightTheme").is_some());
        assert!(read_dword(PERSONALIZE, "ColorPrevalence").is_some());
        assert!(read_dword(DWM, "ColorizationColor").is_some());
        assert!(read_dword(DWM, "AccentColor").is_some());
        let _ = taskbar_is_light();
    }
}
