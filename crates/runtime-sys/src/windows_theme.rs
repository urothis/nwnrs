//! Dark native styling for the classic Win32 NWServer control panel.

#![allow(clippy::missing_safety_doc)]

use std::{
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
    ptr,
    sync::{
        OnceLock,
        atomic::{AtomicPtr, Ordering},
    },
};

use windows_sys::Win32::{
    Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
        },
        Gdi::{
            BeginPaint, CreateSolidBrush, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX,
            DT_SINGLELINE, DT_VCENTER, DrawTextW, EndPaint, FillRect, FrameRect, GetDC, HBRUSH,
            PAINTSTRUCT, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RedrawWindow,
            ReleaseDC, SelectObject, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
        },
    },
    System::{
        LibraryLoader::{GetProcAddress, LoadLibraryW},
        Threading::GetCurrentProcessId,
    },
    UI::{
        Controls::SetWindowTheme,
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            BM_GETCHECK, BM_GETSTATE, BS_3STATE, BS_AUTO3STATE, BS_AUTOCHECKBOX,
            BS_AUTORADIOBUTTON, BS_CHECKBOX, BS_RADIOBUTTON, BS_TYPEMASK, BST_FOCUS, BST_PUSHED,
            CBS_DROPDOWNLIST, CreateWindowExA, CreateWindowExW, EnumChildWindows, GA_ROOT,
            GCLP_HBRBACKGROUND, GWL_STYLE, GetAncestor, GetClassNameW, GetClientRect,
            GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
            HMENU, SendMessageW, SetClassLongPtrW, WM_CTLCOLORBTN, WM_CTLCOLORDLG, WM_CTLCOLOREDIT,
            WM_CTLCOLORLISTBOX, WM_CTLCOLORSCROLLBAR, WM_CTLCOLORSTATIC, WM_ERASEBKGND, WM_GETFONT,
            WM_NCDESTROY, WM_PAINT, WM_SETTINGCHANGE, WM_THEMECHANGED, WS_DISABLED,
        },
    },
};

const ROOT_CLASS: &str = "Exo - BioWare Corp., (c) 1999 - Generic Blank Application";
const ROOT_SUBCLASS_ID: usize = 0x4e57_4e52_5354_484d;
const CONTROL_SUBCLASS_ID: usize = 0x4e57_4e52_5343_544c;
const CONTROL_BUTTON: usize = 1;
const CONTROL_COMBO_BOX: usize = 2;
const CONTROL_SCROLL_BAR: usize = 3;
const ACCENT_HEIGHT: i32 = 2;

const BACKGROUND: COLORREF = rgb(0x15, 0x15, 0x15);
const SURFACE: COLORREF = rgb(0x22, 0x22, 0x22);
const TEXT: COLORREF = rgb(0xee, 0xee, 0xee);
const ACCENT: COLORREF = rgb(0xf2, 0x8c, 0x28);
const BORDER: COLORREF = rgb(0x3d, 0x3d, 0x3d);
const MUTED: COLORREF = rgb(0x88, 0x88, 0x88);

static RESOURCES: OnceLock<ThemeResources> = OnceLock::new();
static CREATE_WINDOW_EX_A_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CREATE_WINDOW_EX_W_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ALLOW_DARK_MODE_FOR_WINDOW: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

type SetPreferredAppModeFunction = unsafe extern "system" fn(i32) -> i32;
type AllowDarkModeForWindowFunction = unsafe extern "system" fn(HWND, i32) -> i32;
type RefreshImmersiveColorPolicyStateFunction = unsafe extern "system" fn();

type CreateWindowExAFunction = unsafe extern "system" fn(
    u32,
    *const u8,
    *const u8,
    u32,
    i32,
    i32,
    i32,
    i32,
    HWND,
    HMENU,
    HINSTANCE,
    *const c_void,
) -> HWND;

type CreateWindowExWFunction = unsafe extern "system" fn(
    u32,
    *const u16,
    *const u16,
    u32,
    i32,
    i32,
    i32,
    i32,
    HWND,
    HMENU,
    HINSTANCE,
    *const c_void,
) -> HWND;

struct ThemeResources {
    background: usize,
    surface:    usize,
    accent:     usize,
    border:     usize,
}

impl ThemeResources {
    fn create() -> Result<Self, String> {
        // SAFETY: CreateSolidBrush receives constant COLORREF values. The
        // brushes intentionally live until process exit because windows may
        // retain them as class or control-color brushes.
        let background = unsafe { CreateSolidBrush(BACKGROUND) };
        // SAFETY: same ownership model as background.
        let surface = unsafe { CreateSolidBrush(SURFACE) };
        // SAFETY: same ownership model as background.
        let accent = unsafe { CreateSolidBrush(ACCENT) };
        // SAFETY: same ownership model as background.
        let border = unsafe { CreateSolidBrush(BORDER) };
        if background.is_null() || surface.is_null() || accent.is_null() || border.is_null() {
            return Err(format!(
                "failed to create Windows theme brushes: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self {
            background: background as usize,
            surface:    surface as usize,
            accent:     accent as usize,
            border:     border as usize,
        })
    }

    fn background(&self) -> HBRUSH {
        self.background as HBRUSH
    }

    fn surface(&self) -> HBRUSH {
        self.surface as HBRUSH
    }

    fn accent(&self) -> HBRUSH {
        self.accent as HBRUSH
    }

    fn border(&self) -> HBRUSH {
        self.border as HBRUSH
    }
}

pub(super) fn install() -> Result<(), String> {
    if !CREATE_WINDOW_EX_W_ORIGINAL
        .load(Ordering::Acquire)
        .is_null()
    {
        return Ok(());
    }
    let resources = ThemeResources::create()?;
    RESOURCES.set(resources).map_err(|_resources| {
        "Windows theme resources were initialized more than once".to_string()
    })?;

    enable_immersive_dark_mode();
    install_create_window_hooks()?;
    tracing::debug!(target: "nwnrs::runtime", "installed Windows dark theme observer");
    Ok(())
}

fn enable_immersive_dark_mode() {
    // Windows exposes these process/control dark-mode entry points by ordinal
    // on supported Windows 10 and 11 builds. They are optional: the explicit
    // DWM, control-theme, and control-color paths below remain the fallback.
    // The module is intentionally retained for the process lifetime.
    // SAFETY: the DLL name is static and null-terminated.
    let theme = unsafe { LoadLibraryW(windows_sys::core::w!("uxtheme.dll")) };
    if theme.is_null() {
        return;
    }
    // SAFETY: a low-valued pointer is the documented MAKEINTRESOURCE encoding
    // used by GetProcAddress for exported ordinals.
    let preferred = unsafe { GetProcAddress(theme, 135_usize as *const u8) };
    if let Some(preferred) = preferred {
        // SAFETY: ordinal 135 is SetPreferredAppMode on supported builds; mode
        // 1 requests AllowDark without forcing applications that opt out.
        let preferred = unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, SetPreferredAppModeFunction>(
                preferred,
            )
        };
        // SAFETY: the resolved function accepts the documented enum value.
        unsafe {
            preferred(1);
        }
    }
    // SAFETY: same ordinal lookup contract as above.
    let allow_window = unsafe { GetProcAddress(theme, 133_usize as *const u8) };
    if let Some(allow_window) = allow_window {
        ALLOW_DARK_MODE_FOR_WINDOW
            .store(allow_window as *const () as *mut c_void, Ordering::Release);
    }
    // SAFETY: ordinal 104 refreshes the immersive color policy and takes no
    // arguments on the supported builds where it is present.
    let refresh = unsafe { GetProcAddress(theme, 104_usize as *const u8) };
    if let Some(refresh) = refresh {
        let refresh = unsafe {
            std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                RefreshImmersiveColorPolicyStateFunction,
            >(refresh)
        };
        // SAFETY: the resolved function takes no arguments.
        unsafe {
            refresh();
        }
    }
}

fn allow_dark_mode(window: HWND) {
    let allow = ALLOW_DARK_MODE_FOR_WINDOW.load(Ordering::Acquire);
    if allow.is_null() {
        return;
    }
    // SAFETY: the slot is populated only from uxtheme ordinal 133 and window
    // is a live HWND returned by CreateWindowEx.
    let allow =
        unsafe { std::mem::transmute::<*mut c_void, AllowDarkModeForWindowFunction>(allow) };
    // SAFETY: true enables dark rendering for this specific control.
    unsafe {
        allow(window, 1);
    }
}

fn install_create_window_hooks() -> Result<(), String> {
    // SAFETY: Gum was initialized before this module is installed and returns
    // one retained interceptor reference.
    let interceptor = unsafe { frida_gum_sys::gum_interceptor_obtain() };
    if interceptor.is_null() {
        return Err("Frida Gum returned no interceptor for Windows theming".to_string());
    }
    let hooks = [
        (
            CreateWindowExA as *const () as *mut c_void,
            create_window_ex_a_replacement as CreateWindowExAFunction as *const () as *mut c_void,
            &CREATE_WINDOW_EX_A_ORIGINAL,
            "CreateWindowExA",
        ),
        (
            CreateWindowExW as *const () as *mut c_void,
            create_window_ex_w_replacement as CreateWindowExWFunction as *const () as *mut c_void,
            &CREATE_WINDOW_EX_W_ORIGINAL,
            "CreateWindowExW",
        ),
    ];
    let mut installed = Vec::new();
    let mut failure = None;
    // SAFETY: each replacement has exactly the target Win32 function's system
    // ABI and signature. The primary NWServer thread is still suspended, so no
    // target call can race publication of the returned trampolines.
    unsafe {
        frida_gum_sys::gum_interceptor_begin_transaction(interceptor);
        for (target, replacement, original_slot, name) in hooks {
            let mut original = ptr::null_mut();
            let status = frida_gum_sys::gum_interceptor_replace(
                interceptor,
                target,
                replacement,
                ptr::null_mut(),
                &raw mut original,
            );
            if status == 0 && !original.is_null() {
                original_slot.store(original, Ordering::Release);
                installed.push((target, original_slot));
            } else {
                failure = Some(format!(
                    "Frida Gum could not observe {name}: status {status}, trampoline {}",
                    if original.is_null() {
                        "missing"
                    } else {
                        "present"
                    }
                ));
                break;
            }
        }
        frida_gum_sys::gum_interceptor_end_transaction(interceptor);
        let _flushed = frida_gum_sys::gum_interceptor_flush(interceptor);
    }
    if let Some(failure) = failure {
        // SAFETY: installed contains only replacements successfully installed
        // by the transaction above.
        unsafe {
            frida_gum_sys::gum_interceptor_begin_transaction(interceptor);
            for (target, original_slot) in installed {
                frida_gum_sys::gum_interceptor_revert(interceptor, target);
                original_slot.store(ptr::null_mut(), Ordering::Release);
            }
            frida_gum_sys::gum_interceptor_end_transaction(interceptor);
            let _flushed = frida_gum_sys::gum_interceptor_flush(interceptor);
        }
        // SAFETY: gum_interceptor_obtain returned one retained reference.
        unsafe { frida_gum_sys::g_object_unref(interceptor.cast()) };
        return Err(failure);
    }
    // SAFETY: installed hooks do not require retaining the interceptor object.
    unsafe { frida_gum_sys::g_object_unref(interceptor.cast()) };
    Ok(())
}

unsafe extern "system" fn create_window_ex_a_replacement(
    extended_style: u32,
    class_name: *const u8,
    window_name: *const u8,
    style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    menu: HMENU,
    instance: HINSTANCE,
    parameter: *const c_void,
) -> HWND {
    let original = CREATE_WINDOW_EX_A_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: Gum returned a CreateWindowExA trampoline for this exact target.
    let original = unsafe { std::mem::transmute::<*mut c_void, CreateWindowExAFunction>(original) };
    // SAFETY: all arguments are forwarded without modification.
    let window = unsafe {
        original(
            extended_style,
            class_name,
            window_name,
            style,
            x,
            y,
            width,
            height,
            parent,
            menu,
            instance,
            parameter,
        )
    };
    theme_after_creation(window);
    window
}

unsafe extern "system" fn create_window_ex_w_replacement(
    extended_style: u32,
    class_name: *const u16,
    window_name: *const u16,
    style: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    menu: HMENU,
    instance: HINSTANCE,
    parameter: *const c_void,
) -> HWND {
    let original = CREATE_WINDOW_EX_W_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: Gum returned a CreateWindowExW trampoline for this exact target.
    let original = unsafe { std::mem::transmute::<*mut c_void, CreateWindowExWFunction>(original) };
    // SAFETY: all arguments are forwarded without modification.
    let window = unsafe {
        original(
            extended_style,
            class_name,
            window_name,
            style,
            x,
            y,
            width,
            height,
            parent,
            menu,
            instance,
            parameter,
        )
    };
    theme_after_creation(window);
    window
}

fn theme_after_creation(window: HWND) {
    if window.is_null() {
        return;
    }
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: CreateWindowEx returned this live window on its owning thread.
        unsafe { theme_created_window(window) };
    }));
}

unsafe fn theme_created_window(window: HWND) {
    let mut process_id = 0_u32;
    // SAFETY: window is provided by the in-context event callback and the
    // process-id output is writable.
    unsafe { GetWindowThreadProcessId(window, &raw mut process_id) };
    if process_id != unsafe { GetCurrentProcessId() } {
        return;
    }

    let class = class_name(window);
    if class == ROOT_CLASS {
        // SAFETY: this is the NWServer root window on its owning thread.
        unsafe { theme_root(window) };
        return;
    }

    // ComboLBox popup windows are top-level even though their logical owner is
    // a ComboBox, so accept that known class directly. Other controls must be
    // descendants of the exact NWServer root class.
    let root = unsafe { GetAncestor(window, GA_ROOT) };
    if class != "ComboLBox" && (root.is_null() || class_name(root) != ROOT_CLASS) {
        return;
    }
    // SAFETY: the HWND is an NWServer control or one of its combo popups.
    unsafe { theme_control(window, &class) };
}

unsafe fn theme_root(window: HWND) {
    let Some(resources) = RESOURCES.get() else {
        return;
    };
    let dark = 1_i32;
    let caption_color = BACKGROUND;
    let text_color = TEXT;
    let border_color = ACCENT;
    allow_dark_mode(window);
    // SAFETY: each DWM call receives a live top-level HWND and a correctly
    // sized value for its attribute. Unsupported attributes simply fail.
    unsafe {
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&raw const dark).cast(),
            size_u32::<i32>(),
        );
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_CAPTION_COLOR as u32,
            (&raw const caption_color).cast(),
            size_u32::<COLORREF>(),
        );
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_TEXT_COLOR as u32,
            (&raw const text_color).cast(),
            size_u32::<COLORREF>(),
        );
        let _ = DwmSetWindowAttribute(
            window,
            DWMWA_BORDER_COLOR as u32,
            (&raw const border_color).cast(),
            size_u32::<COLORREF>(),
        );
        let _ = SetWindowTheme(
            window,
            windows_sys::core::w!("DarkMode_Explorer"),
            ptr::null(),
        );
        SetClassLongPtrW(window, GCLP_HBRBACKGROUND, resources.background() as isize);
        let _ = SetWindowSubclass(window, Some(root_subclass), ROOT_SUBCLASS_ID, 0);
        let _ = EnumChildWindows(window, Some(theme_child_callback), 0);
        let _ = RedrawWindow(
            window,
            ptr::null(),
            ptr::null_mut(),
            RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN,
        );
    }
}

unsafe extern "system" fn theme_child_callback(window: HWND, _parameter: LPARAM) -> i32 {
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        let class = class_name(window);
        // SAFETY: EnumChildWindows supplied a live descendant on the root
        // window's owning thread.
        unsafe { theme_control(window, &class) };
    }));
    1
}

unsafe fn theme_control(window: HWND, class: &str) {
    allow_dark_mode(window);
    let theme = match class {
        "Edit" => windows_sys::core::w!("DarkMode_CFD"),
        "Button" | "ComboBox" | "ComboLBox" | "ListBox" | "ScrollBar" => {
            windows_sys::core::w!("DarkMode_Explorer")
        }
        _ => ptr::null(),
    };
    if !theme.is_null() {
        // SAFETY: window is a live native control and the theme name is static.
        unsafe {
            let _ = SetWindowTheme(window, theme, ptr::null());
        }
    }
    let control_kind = match class {
        "Button" => Some(CONTROL_BUTTON),
        "ComboBox" if window_style(window) & 0x3 == CBS_DROPDOWNLIST as u32 => {
            Some(CONTROL_COMBO_BOX)
        }
        "ScrollBar" => Some(CONTROL_SCROLL_BAR),
        _ => None,
    };
    if let Some(control_kind) = control_kind {
        // SAFETY: CreateWindowEx returned on this control's owning thread and
        // the callback remains in the loaded runtime DLL.
        unsafe {
            let _ = SetWindowSubclass(
                window,
                Some(control_subclass),
                CONTROL_SUBCLASS_ID,
                control_kind,
            );
        }
    }
    // SAFETY: invalidation uses no caller-owned pointers and causes the control
    // to repaint with its newly selected theme.
    unsafe {
        let _ = RedrawWindow(
            window,
            ptr::null(),
            ptr::null_mut(),
            RDW_INVALIDATE | RDW_ERASE | RDW_FRAME,
        );
    }
}

unsafe extern "system" fn control_subclass(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    control_kind: usize,
) -> LRESULT {
    panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: ComCtl32 supplied the active subclass parameters.
        unsafe { control_subclass_inner(window, message, wparam, lparam, control_kind) }
    }))
    .unwrap_or_else(|_payload| {
        // SAFETY: preserve the original control on any painting failure.
        unsafe { DefSubclassProc(window, message, wparam, lparam) }
    })
}

unsafe fn control_subclass_inner(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    control_kind: usize,
) -> LRESULT {
    match message {
        WM_PAINT => {
            // SAFETY: each painter pairs BeginPaint/EndPaint for its live HWND.
            unsafe {
                match control_kind {
                    CONTROL_BUTTON => paint_button(window),
                    CONTROL_COMBO_BOX => paint_combo_box(window),
                    CONTROL_SCROLL_BAR => paint_scroll_bar(window),
                    _ => return DefSubclassProc(window, message, wparam, lparam),
                }
            }
            0
        }
        WM_NCDESTROY => {
            // SAFETY: complete normal destruction before removing this exact
            // subclass entry.
            let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
            unsafe {
                RemoveWindowSubclass(window, Some(control_subclass), CONTROL_SUBCLASS_ID);
            }
            result
        }
        _ => unsafe { DefSubclassProc(window, message, wparam, lparam) },
    }
}

unsafe fn paint_button(window: HWND) {
    let Some(resources) = RESOURCES.get() else {
        return;
    };
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: window is in WM_PAINT and paint is writable.
    let dc = unsafe { BeginPaint(window, &raw mut paint) };
    if dc.is_null() {
        return;
    }
    let mut bounds = RECT::default();
    // SAFETY: window is live and bounds is writable.
    unsafe { GetClientRect(window, &raw mut bounds) };
    let style = window_style(window);
    let button_type = style & BS_TYPEMASK as u32;
    let checkbox = matches!(
        button_type as i32,
        BS_CHECKBOX
            | BS_AUTOCHECKBOX
            | BS_RADIOBUTTON
            | BS_3STATE
            | BS_AUTO3STATE
            | BS_AUTORADIOBUTTON
    );
    if checkbox {
        // SAFETY: the HDC and process-lifetime brushes are valid.
        unsafe { paint_checkbox(window, dc, bounds, resources) };
    } else {
        // SAFETY: the HDC and process-lifetime brushes are valid.
        unsafe { paint_push_button(window, dc, bounds, resources) };
    }
    // SAFETY: pairs the successful BeginPaint above.
    unsafe { EndPaint(window, &raw const paint) };
}

unsafe fn paint_push_button(
    window: HWND,
    dc: *mut c_void,
    mut bounds: RECT,
    resources: &ThemeResources,
) {
    // SAFETY: standard button messages return immediate scalar state.
    let state =
        u32::try_from(unsafe { SendMessageW(window, BM_GETSTATE, 0, 0) }).unwrap_or_default();
    let style = window_style(window);
    let pressed = state & BST_PUSHED != 0;
    let focused = state & BST_FOCUS != 0;
    // SAFETY: the HDC and brushes are valid for this paint cycle.
    unsafe {
        FillRect(
            dc,
            &raw const bounds,
            if pressed {
                resources.background()
            } else {
                resources.surface()
            },
        );
        FrameRect(
            dc,
            &raw const bounds,
            if focused || pressed {
                resources.accent()
            } else {
                resources.border()
            },
        );
    }
    bounds.left += 5;
    bounds.right -= 5;
    // SAFETY: dc belongs to window and bounds is its client rectangle.
    unsafe {
        draw_window_text(
            window,
            dc,
            &mut bounds,
            if style & WS_DISABLED != 0 {
                MUTED
            } else {
                TEXT
            },
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
}

unsafe fn paint_checkbox(window: HWND, dc: *mut c_void, bounds: RECT, resources: &ThemeResources) {
    // SAFETY: the HDC and brushes are valid for this paint cycle.
    unsafe { FillRect(dc, &raw const bounds, resources.background()) };
    let size = (bounds.bottom - bounds.top - 4).clamp(9, 14);
    let top = bounds.top + ((bounds.bottom - bounds.top - size) / 2);
    let box_bounds = RECT {
        left: bounds.left + 2,
        top,
        right: bounds.left + 2 + size,
        bottom: top + size,
    };
    // SAFETY: standard button message returns immediate check state.
    let checked = unsafe { SendMessageW(window, BM_GETCHECK, 0, 0) } != 0;
    // SAFETY: dc and all brushes are valid.
    unsafe {
        FillRect(dc, &raw const box_bounds, resources.surface());
        FrameRect(
            dc,
            &raw const box_bounds,
            if checked {
                resources.accent()
            } else {
                resources.border()
            },
        );
        if checked {
            let inner = RECT {
                left:   box_bounds.left + 3,
                top:    box_bounds.top + 3,
                right:  box_bounds.right - 3,
                bottom: box_bounds.bottom - 3,
            };
            FillRect(dc, &raw const inner, resources.accent());
        }
    }
    let mut text_bounds = bounds;
    text_bounds.left = box_bounds.right + 6;
    let style = window_style(window);
    // SAFETY: dc belongs to window and text_bounds is inside its client area.
    unsafe {
        draw_window_text(
            window,
            dc,
            &mut text_bounds,
            if style & WS_DISABLED != 0 {
                MUTED
            } else {
                TEXT
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
}

unsafe fn paint_combo_box(window: HWND) {
    let Some(resources) = RESOURCES.get() else {
        return;
    };
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: window is in WM_PAINT and paint is writable.
    let dc = unsafe { BeginPaint(window, &raw mut paint) };
    if dc.is_null() {
        return;
    }
    let mut bounds = RECT::default();
    // SAFETY: bounds is writable and dc/brushes are valid.
    unsafe {
        GetClientRect(window, &raw mut bounds);
        FillRect(dc, &raw const bounds, resources.surface());
        FrameRect(dc, &raw const bounds, resources.border());
    }
    let arrow_width = (bounds.bottom - bounds.top).clamp(18, 24);
    let mut text_bounds = bounds;
    text_bounds.left += 6;
    text_bounds.right -= arrow_width + 3;
    let style = window_style(window);
    // SAFETY: dc belongs to window and text_bounds is writable.
    unsafe {
        draw_window_text(
            window,
            dc,
            &mut text_bounds,
            if style & WS_DISABLED != 0 {
                MUTED
            } else {
                TEXT
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
    let mut arrow_bounds = bounds;
    arrow_bounds.left = bounds.right - arrow_width;
    arrow_bounds.right -= 1;
    // SAFETY: the static glyph and arrow_bounds are valid for DrawTextW.
    unsafe {
        SetTextColor(dc, ACCENT);
        SetBkMode(dc, TRANSPARENT as i32);
        DrawTextW(
            dc,
            windows_sys::core::w!("v"),
            1,
            &raw mut arrow_bounds,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        EndPaint(window, &raw const paint);
    }
}

unsafe fn paint_scroll_bar(window: HWND) {
    let Some(resources) = RESOURCES.get() else {
        return;
    };
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: window is in WM_PAINT and paint is writable.
    let dc = unsafe { BeginPaint(window, &raw mut paint) };
    if dc.is_null() {
        return;
    }
    let mut bounds = RECT::default();
    // SAFETY: bounds is writable and the paint resources are valid.
    unsafe {
        GetClientRect(window, &raw mut bounds);
        FillRect(dc, &raw const bounds, resources.surface());
        FrameRect(dc, &raw const bounds, resources.border());
        SetTextColor(dc, ACCENT);
        SetBkMode(dc, TRANSPARENT as i32);
    }
    if bounds.bottom - bounds.top >= bounds.right - bounds.left {
        let middle = bounds.top + ((bounds.bottom - bounds.top) / 2);
        let mut first = bounds;
        first.bottom = middle;
        let mut second = bounds;
        second.top = middle;
        // SAFETY: static glyphs and rectangles are valid.
        unsafe {
            DrawTextW(
                dc,
                windows_sys::core::w!("^"),
                1,
                &raw mut first,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            DrawTextW(
                dc,
                windows_sys::core::w!("v"),
                1,
                &raw mut second,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    } else {
        let middle = bounds.left + ((bounds.right - bounds.left) / 2);
        let mut first = bounds;
        first.right = middle;
        let mut second = bounds;
        second.left = middle;
        // SAFETY: static glyphs and rectangles are valid.
        unsafe {
            DrawTextW(
                dc,
                windows_sys::core::w!("<"),
                1,
                &raw mut first,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            DrawTextW(
                dc,
                windows_sys::core::w!(">"),
                1,
                &raw mut second,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    }
    // SAFETY: pairs the successful BeginPaint.
    unsafe { EndPaint(window, &raw const paint) };
}

unsafe fn draw_window_text(
    window: HWND,
    dc: *mut c_void,
    bounds: &mut RECT,
    color: COLORREF,
    format: u32,
) {
    let text = window_text(window);
    // SAFETY: standard WM_GETFONT returns a borrowed GDI font handle.
    let font = unsafe { SendMessageW(window, WM_GETFONT, 0, 0) } as *mut c_void;
    let previous = if font.is_null() {
        ptr::null_mut()
    } else {
        // SAFETY: font is a borrowed GDI object and dc is active.
        unsafe { SelectObject(dc, font) }
    };
    // SAFETY: text and bounds remain valid for the draw call.
    unsafe {
        SetTextColor(dc, color);
        SetBkMode(dc, TRANSPARENT as i32);
        DrawTextW(
            dc,
            text.as_ptr(),
            i32::try_from(text.len()).unwrap_or(i32::MAX),
            bounds,
            format,
        );
        if !previous.is_null() {
            SelectObject(dc, previous);
        }
    }
}

fn window_text(window: HWND) -> Vec<u16> {
    // SAFETY: window is live and the call does not retain pointers.
    let length = unsafe { GetWindowTextLengthW(window) }.max(0) as usize;
    let mut text = vec![0_u16; length.saturating_add(1)];
    // SAFETY: text is writable for its declared length.
    let copied = unsafe {
        GetWindowTextW(
            window,
            text.as_mut_ptr(),
            i32::try_from(text.len()).unwrap_or(i32::MAX),
        )
    };
    text.truncate(usize::try_from(copied).unwrap_or(0));
    text
}

unsafe extern "system" fn root_subclass(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: ComCtl32 invokes this callback with the active subclass
        // parameters for window.
        unsafe { root_subclass_inner(window, message, wparam, lparam) }
    }))
    .unwrap_or_else(|_payload| {
        // SAFETY: falling through preserves NWServer behavior after a panic.
        unsafe { DefSubclassProc(window, message, wparam, lparam) }
    })
}

unsafe fn root_subclass_inner(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let Some(resources) = RESOURCES.get() else {
        // SAFETY: this is the current subclass chain.
        return unsafe { DefSubclassProc(window, message, wparam, lparam) };
    };
    match message {
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLORDLG => {
            let dc = wparam as *mut _;
            // SAFETY: control-color messages provide a live HDC in wParam.
            unsafe {
                SetTextColor(dc, TEXT);
                SetBkColor(dc, BACKGROUND);
                SetBkMode(dc, TRANSPARENT as i32);
            }
            resources.background() as LRESULT
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let dc = wparam as *mut _;
            // SAFETY: control-color messages provide a live HDC in wParam.
            unsafe {
                SetTextColor(dc, TEXT);
                SetBkColor(dc, SURFACE);
            }
            resources.surface() as LRESULT
        }
        WM_CTLCOLORSCROLLBAR => resources.surface() as LRESULT,
        WM_ERASEBKGND => {
            let mut bounds = RECT::default();
            // SAFETY: wParam is the erase HDC and bounds is writable.
            unsafe {
                if GetClientRect(window, &raw mut bounds) != 0 {
                    FillRect(wparam as *mut _, &raw const bounds, resources.background());
                    return 1;
                }
            }
            0
        }
        WM_PAINT => {
            // SAFETY: let NWServer validate and paint its client region first.
            let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
            // SAFETY: GetDC/ReleaseDC are paired for this live window.
            unsafe { draw_accent(window, resources.accent()) };
            result
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            // SAFETY: reapplying the supported DWM/theme attributes is
            // idempotent and preserves the active subclass.
            unsafe { theme_root(window) };
            unsafe { DefSubclassProc(window, message, wparam, lparam) }
        }
        WM_NCDESTROY => {
            // SAFETY: complete normal destruction before removing this exact
            // subclass entry from the live chain.
            let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
            unsafe {
                RemoveWindowSubclass(window, Some(root_subclass), ROOT_SUBCLASS_ID);
            }
            result
        }
        _ => unsafe { DefSubclassProc(window, message, wparam, lparam) },
    }
}

unsafe fn draw_accent(window: HWND, brush: HBRUSH) {
    // SAFETY: caller provides a live root window and process-lifetime brush.
    let dc = unsafe { GetDC(window) };
    if dc.is_null() {
        return;
    }
    let mut bounds = RECT::default();
    // SAFETY: bounds is writable and dc belongs to window.
    unsafe {
        if GetClientRect(window, &raw mut bounds) != 0 {
            bounds.bottom = (bounds.top + ACCENT_HEIGHT).min(bounds.bottom);
            FillRect(dc, &raw const bounds, brush);
        }
        ReleaseDC(window, dc);
    }
}

fn class_name(window: HWND) -> String {
    let mut buffer = [0_u16; 128];
    // SAFETY: buffer is writable and window came from USER32 enumeration.
    let length = unsafe {
        GetClassNameW(
            window,
            buffer.as_mut_ptr(),
            i32::try_from(buffer.len()).unwrap_or(i32::MAX),
        )
    };
    let length = usize::try_from(length).unwrap_or(0);
    String::from_utf16_lossy(buffer.get(..length).unwrap_or_default())
}

const fn rgb(red: u32, green: u32, blue: u32) -> COLORREF {
    red | (green << 8) | (blue << 16)
}

fn size_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}

fn window_style(window: HWND) -> u32 {
    // SAFETY: window is a live HWND and GWL_STYLE returns an immediate value.
    u32::try_from(unsafe { GetWindowLongPtrW(window, GWL_STYLE) }).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_uses_windows_colorref_byte_order() {
        assert_eq!(BACKGROUND, 0x0015_1515);
        assert_eq!(SURFACE, 0x0022_2222);
        assert_eq!(TEXT, 0x00ee_eeee);
        assert_eq!(ACCENT, 0x0028_8cf2);
    }
}
