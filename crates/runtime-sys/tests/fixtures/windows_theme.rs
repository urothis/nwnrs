//! Native Windows fixture for the NWServer control-panel dark theme.

use std::{ffi::c_void, ptr};

type Handle = *mut c_void;
type Window = Handle;
type Instance = Handle;
type Brush = Handle;
type Cursor = Handle;
type Icon = Handle;
type Menu = Handle;
type DeviceContext = Handle;

type WindowProcedure = Option<unsafe extern "system" fn(Window, u32, usize, isize) -> isize>;

#[repr(C)]
struct WindowClass {
    style:          u32,
    procedure:      WindowProcedure,
    class_extra:    i32,
    window_extra:   i32,
    instance:       Instance,
    icon:           Icon,
    cursor:         Cursor,
    background:     Brush,
    menu_name:      *const u16,
    class_name:     *const u16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Instance;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WindowClass) -> u16;
    fn CreateWindowExW(
        extended_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Window,
        menu: Menu,
        instance: Instance,
        parameter: *const c_void,
    ) -> Window;
    fn DefWindowProcW(window: Window, message: u32, wparam: usize, lparam: isize) -> isize;
    fn DestroyWindow(window: Window) -> i32;
    fn GetClassLongPtrW(window: Window, index: i32) -> usize;
    fn GetDC(window: Window) -> DeviceContext;
    fn ReleaseDC(window: Window, context: DeviceContext) -> i32;
    fn SendMessageW(window: Window, message: u32, wparam: usize, lparam: isize) -> isize;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmGetWindowAttribute(
        window: Window,
        attribute: u32,
        value: *mut c_void,
        value_size: u32,
    ) -> i32;
}

const ROOT_CLASS: &str = "Exo - BioWare Corp., (c) 1999 - Generic Blank Application";
const WS_OVERLAPPEDWINDOW: u32 = 0x00cf_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const BS_AUTOCHECKBOX: u32 = 3;
const GCLP_HBRBACKGROUND: i32 = -10;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const WM_CTLCOLORSTATIC: u32 = 312;
const BM_GETCHECK: u32 = 240;
const BM_CLICK: u32 = 245;

pub(super) fn verify() {
    let root_class = wide(ROOT_CLASS);
    // SAFETY: null requests the current executable module.
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    assert!(!instance.is_null(), "get Windows fixture module");
    let class = WindowClass {
        style: 0,
        procedure: Some(window_procedure),
        class_extra: 0,
        window_extra: 0,
        instance,
        icon: ptr::null_mut(),
        cursor: ptr::null_mut(),
        background: ptr::null_mut(),
        menu_name: ptr::null(),
        class_name: root_class.as_ptr(),
    };
    // SAFETY: class and its UTF-16 name remain live for the registration call.
    assert_ne!(unsafe { RegisterClassW(&raw const class) }, 0);

    // SAFETY: all class/title strings remain live and the fixture owns every
    // returned window until DestroyWindow.
    let root = unsafe {
        CreateWindowExW(
            0,
            root_class.as_ptr(),
            wide("NWServer theme fixture").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            480,
            320,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    assert!(!root.is_null(), "create themed NWServer root fixture");
    let label = create_control(instance, root, "Static", "Fixture label", WS_VISIBLE);
    let checkbox = create_control(
        instance,
        root,
        "Button",
        "Fixture option",
        WS_VISIBLE | BS_AUTOCHECKBOX,
    );
    let _combo = create_control(instance, root, "ComboBox", "Fixture choice", WS_VISIBLE | 3);

    let mut dark = 0_i32;
    // SAFETY: root is live and dark is writable with the supplied size.
    assert_eq!(
        unsafe {
            DwmGetWindowAttribute(
                root,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw mut dark).cast(),
                size_of::<i32>() as u32,
            )
        },
        0
    );
    assert_eq!(dark, 1, "runtime enabled immersive dark mode");
    // SAFETY: root is live and the class brush is a borrowed handle.
    let root_brush = unsafe { GetClassLongPtrW(root, GCLP_HBRBACKGROUND) };
    assert_ne!(root_brush, 0, "runtime installed the dark root brush");
    // SAFETY: label is live; the parent synchronously services this control
    // color request and returns its process-lifetime brush.
    let context = unsafe { GetDC(label) };
    assert!(!context.is_null());
    let label_brush = unsafe {
        SendMessageW(
            root,
            WM_CTLCOLORSTATIC,
            context as usize,
            label as isize,
        )
    };
    // SAFETY: context was acquired for label.
    unsafe { ReleaseDC(label, context) };
    assert_eq!(label_brush as usize, root_brush);
    // The control subclass paints only. Default button behavior must remain
    // intact underneath it.
    // SAFETY: synchronous scalar button messages on the live fixture control.
    assert_eq!(unsafe { SendMessageW(checkbox, BM_GETCHECK, 0, 0) }, 0);
    unsafe { SendMessageW(checkbox, BM_CLICK, 0, 0) };
    assert_ne!(unsafe { SendMessageW(checkbox, BM_GETCHECK, 0, 0) }, 0);

    // SAFETY: destroying the root also destroys its owned child controls.
    assert_ne!(unsafe { DestroyWindow(root) }, 0);
}

fn create_control(
    instance: Instance,
    parent: Window,
    class: &str,
    text: &str,
    style: u32,
) -> Window {
    let class = wide(class);
    let text = wide(text);
    // SAFETY: strings live through the call and parent/instance are owned by
    // this fixture process.
    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | style,
            8,
            8,
            180,
            24,
            parent,
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    assert!(!window.is_null(), "create Windows theme control {class:?}");
    window
}

unsafe extern "system" fn window_procedure(
    window: Window,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    // SAFETY: this forwards the exact parameters supplied by USER32.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
