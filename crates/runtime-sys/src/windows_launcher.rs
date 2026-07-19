//! Safe launcher surface over the Windows process-injection boundary.

use std::{
    error::Error,
    ffi::{OsStr, c_void},
    fmt, mem,
    os::windows::{ffi::OsStrExt as _, io::AsRawHandle as _, process::CommandExt as _},
    path::Path,
    process::{Child, Command},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, FreeLibrary, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    System::{
        Diagnostics::{
            Debug::WriteProcessMemory,
            ToolHelp::{
                CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPTHREAD, THREADENTRY32,
                Thread32First, Thread32Next,
            },
        },
        LibraryLoader::{
            DONT_RESOLVE_DLL_REFERENCES, GetModuleHandleW, GetProcAddress, LoadLibraryExW,
        },
        Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
        },
        Threading::{
            CREATE_SUSPENDED, CreateRemoteThread, GetExitCodeThread, OpenThread, ResumeThread,
            THREAD_SUSPEND_RESUME, WaitForSingleObject,
        },
    },
    UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT},
};

const REMOTE_THREAD_TIMEOUT_MS: u32 = 30_000;
const RUNTIME_INITIALIZER: &[u8] = b"nwnrs_runtime_initialize\0";

/// An error produced while starting and initializing an injected Windows
/// process.
#[derive(Debug)]
pub struct WindowsLaunchError {
    message: String,
}

impl WindowsLaunchError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn windows(operation: &str) -> Self {
        Self::new(format!(
            "{operation} failed: {}",
            std::io::Error::last_os_error()
        ))
    }
}

impl fmt::Display for WindowsLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WindowsLaunchError {}

/// Control operations retained for one initialized Windows server process.
#[derive(Clone, Copy, Debug)]
pub struct WindowsProcessControl {
    primary_thread_id: u32,
}

impl WindowsProcessControl {
    /// Posts `WM_QUIT` to the server's primary thread message queue.
    ///
    /// # Errors
    ///
    /// Returns an error when Windows rejects the thread message.
    pub fn request_graceful_shutdown(self) -> Result<(), WindowsLaunchError> {
        // SAFETY: the thread identifier belongs to the child created by
        // spawn_injected_windows. WM_QUIT carries no pointer-valued payload.
        if unsafe { PostThreadMessageW(self.primary_thread_id, WM_QUIT, 0, 0) } == 0 {
            return Err(WindowsLaunchError::windows("PostThreadMessageW(WM_QUIT)"));
        }
        Ok(())
    }
}

/// Starts a Windows process suspended, injects and initializes the runtime
/// DLL, and resumes its primary thread only after initialization succeeds.
///
/// The caller configures arguments, environment, working directory, and
/// standard-I/O pipes on `command` before calling this function.
///
/// # Errors
///
/// Returns an error when the process cannot be created, the DLL cannot be
/// injected, the exported runtime initializer fails, or the primary thread
/// cannot be resumed. A child that fails before resume is terminated and
/// reaped before this function returns.
pub fn spawn_injected_windows(
    command: &mut Command,
    runtime: &Path,
) -> Result<(Child, WindowsProcessControl), WindowsLaunchError> {
    let runtime = runtime.canonicalize().map_err(|error| {
        WindowsLaunchError::new(format!(
            "failed to resolve runtime DLL {}: {error}",
            runtime.display()
        ))
    })?;
    if !runtime.is_file() {
        return Err(WindowsLaunchError::new(format!(
            "runtime DLL is not a file: {}",
            runtime.display()
        )));
    }

    command.creation_flags(CREATE_SUSPENDED);
    let mut child = command.spawn().map_err(|error| {
        WindowsLaunchError::new(format!(
            "failed to create suspended server process: {error}"
        ))
    })?;
    let result = initialize_suspended_process(&child, &runtime);
    match result {
        Ok(control) => Ok((child, control)),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}

fn initialize_suspended_process(
    child: &Child,
    runtime: &Path,
) -> Result<WindowsProcessControl, WindowsLaunchError> {
    let process_id = child.id();
    let process = child.as_raw_handle().cast::<c_void>();
    let (primary_thread_id, primary_thread) = open_only_process_thread(process_id)?;
    inject_library(process, process_id, runtime)?;
    invoke_runtime_initializer(process, process_id, runtime)?;

    // SAFETY: this handle names the one suspended primary thread found before
    // any remote injection threads were created.
    let previous_count = unsafe { ResumeThread(primary_thread.get()) };
    if previous_count == u32::MAX {
        return Err(WindowsLaunchError::windows("ResumeThread"));
    }
    if previous_count != 1 {
        return Err(WindowsLaunchError::new(format!(
            "server primary thread had unexpected suspend count {previous_count}"
        )));
    }
    Ok(WindowsProcessControl {
        primary_thread_id,
    })
}

fn open_only_process_thread(process_id: u32) -> Result<(u32, OwnedHandle), WindowsLaunchError> {
    let snapshot = create_snapshot(TH32CS_SNAPTHREAD, 0, "thread snapshot")?;
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(mem::size_of::<THREADENTRY32>())
            .map_err(|_error| WindowsLaunchError::new("THREADENTRY32 size exceeds u32"))?,
        ..THREADENTRY32::default()
    };
    let mut matches = Vec::new();
    // SAFETY: snapshot is valid and entry points to initialized writable storage.
    let mut available = unsafe { Thread32First(snapshot.get(), &raw mut entry) } != 0;
    while available {
        if entry.th32OwnerProcessID == process_id {
            matches.push(entry.th32ThreadID);
        }
        // SAFETY: snapshot and entry remain valid for enumeration.
        available = unsafe { Thread32Next(snapshot.get(), &raw mut entry) } != 0;
    }
    let [thread_id] = matches.as_slice() else {
        return Err(WindowsLaunchError::new(format!(
            "suspended server process {process_id} has {} threads; expected exactly one",
            matches.len()
        )));
    };
    // SAFETY: the enumerated identifier belongs to the live suspended child.
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, *thread_id) };
    let thread = OwnedHandle::new(thread, "OpenThread")?;
    Ok((*thread_id, thread))
}

fn inject_library(
    process: HANDLE,
    process_id: u32,
    runtime: &Path,
) -> Result<(), WindowsLaunchError> {
    let runtime_wide = wide_null(runtime.as_os_str());
    let allocation = RemoteAllocation::new(
        process,
        runtime_wide.len().saturating_mul(mem::size_of::<u16>()),
    )?;
    let mut written = 0_usize;
    // SAFETY: allocation is writable remote storage and runtime_wide remains
    // live for the complete copy.
    let copied = unsafe {
        WriteProcessMemory(
            process,
            allocation.get(),
            runtime_wide.as_ptr().cast(),
            allocation.size,
            &raw mut written,
        )
    };
    if copied == 0 || written != allocation.size {
        return Err(WindowsLaunchError::windows(
            "WriteProcessMemory(runtime path)",
        ));
    }

    let kernel32 = wide_null(OsStr::new("kernel32.dll"));
    // SAFETY: kernel32 is loaded in the launcher and the name is terminated.
    let local_kernel32 = unsafe { GetModuleHandleW(kernel32.as_ptr()) };
    if local_kernel32.is_null() {
        return Err(WindowsLaunchError::windows(
            "GetModuleHandleW(kernel32.dll)",
        ));
    }
    // SAFETY: module is valid and the ASCII procedure name is terminated.
    let local_load_library =
        unsafe { GetProcAddress(local_kernel32, c"LoadLibraryW".as_ptr().cast()) }
            .ok_or_else(|| WindowsLaunchError::windows("GetProcAddress(LoadLibraryW)"))?;
    let load_library_rva = (local_load_library as usize)
        .checked_sub(local_kernel32 as usize)
        .ok_or_else(|| WindowsLaunchError::new("LoadLibraryW address precedes kernel32 base"))?;
    // Windows maps the native system DLLs at the same address in same-bitness
    // processes for a boot session. A just-created suspended process has not
    // initialized the loader list yet, so its modules cannot be enumerated
    // until LoadLibraryW has run once.
    let remote_load_library = (local_kernel32 as usize)
        .checked_add(load_library_rva)
        .ok_or_else(|| WindowsLaunchError::new("remote LoadLibraryW address overflowed"))?;
    run_remote_thread(
        process,
        remote_load_library,
        allocation.get(),
        "LoadLibraryW",
    )?;

    let runtime_name = runtime
        .file_name()
        .ok_or_else(|| WindowsLaunchError::new("runtime DLL has no filename"))?;
    let _loaded_base = module_base(process_id, runtime_name)?;
    Ok(())
}

fn invoke_runtime_initializer(
    process: HANDLE,
    process_id: u32,
    runtime: &Path,
) -> Result<(), WindowsLaunchError> {
    let runtime_wide = wide_null(runtime.as_os_str());
    // SAFETY: the canonical path is terminated. DONT_RESOLVE_DLL_REFERENCES
    // maps the image only for export-RVA inspection and does not run DllMain.
    let local_runtime = unsafe {
        LoadLibraryExW(
            runtime_wide.as_ptr(),
            ptr::null_mut(),
            DONT_RESOLVE_DLL_REFERENCES,
        )
    };
    if local_runtime.is_null() {
        return Err(WindowsLaunchError::windows(
            "LoadLibraryExW(runtime inspection)",
        ));
    }
    let initializer_rva = (|| {
        // SAFETY: the mapped image is valid and the export name is terminated.
        let initializer = unsafe { GetProcAddress(local_runtime, RUNTIME_INITIALIZER.as_ptr()) }
            .ok_or_else(|| {
                WindowsLaunchError::windows("GetProcAddress(nwnrs_runtime_initialize)")
            })?;
        (initializer as usize)
            .checked_sub(local_runtime as usize)
            .ok_or_else(|| WindowsLaunchError::new("runtime initializer precedes module base"))
    })();
    // SAFETY: local_runtime was returned by LoadLibraryExW in this process.
    unsafe {
        FreeLibrary(local_runtime);
    }
    let initializer_rva = initializer_rva?;
    let runtime_name = runtime
        .file_name()
        .ok_or_else(|| WindowsLaunchError::new("runtime DLL has no filename"))?;
    let remote_runtime = module_base(process_id, runtime_name)?;
    let remote_initializer = remote_runtime
        .checked_add(initializer_rva)
        .ok_or_else(|| WindowsLaunchError::new("remote initializer address overflowed"))?;
    let exit_code = run_remote_thread(
        process,
        remote_initializer,
        ptr::null_mut(),
        "nwnrs_runtime_initialize",
    )?;
    if exit_code != 0 {
        return Err(WindowsLaunchError::new(format!(
            "runtime initializer returned status {exit_code}"
        )));
    }
    Ok(())
}

fn run_remote_thread(
    process: HANDLE,
    address: usize,
    parameter: *mut c_void,
    name: &str,
) -> Result<u32, WindowsLaunchError> {
    // SAFETY: the caller resolved address in the target process to a function
    // having the LPTHREAD_START_ROUTINE ABI. parameter points to target-process
    // storage or is null as required by that function.
    let start =
        unsafe { mem::transmute::<usize, unsafe extern "system" fn(*mut c_void) -> u32>(address) };
    // SAFETY: process is a live child handle and start/parameter are described
    // above.
    let thread = unsafe {
        CreateRemoteThread(
            process,
            ptr::null(),
            0,
            Some(start),
            parameter,
            0,
            ptr::null_mut(),
        )
    };
    let thread = OwnedHandle::new(thread, &format!("CreateRemoteThread({name})"))?;
    // SAFETY: thread is a valid synchronization handle.
    let wait = unsafe { WaitForSingleObject(thread.get(), REMOTE_THREAD_TIMEOUT_MS) };
    if wait != WAIT_OBJECT_0 {
        return Err(WindowsLaunchError::new(format!(
            "remote {name} thread did not complete within {REMOTE_THREAD_TIMEOUT_MS} ms (wait \
             {wait})"
        )));
    }
    let mut exit_code = 0_u32;
    // SAFETY: the thread completed and exit_code is writable.
    if unsafe { GetExitCodeThread(thread.get(), &raw mut exit_code) } == 0 {
        return Err(WindowsLaunchError::windows("GetExitCodeThread"));
    }
    Ok(exit_code)
}

fn module_base(process_id: u32, name: &OsStr) -> Result<usize, WindowsLaunchError> {
    let snapshot = create_snapshot(
        TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32,
        process_id,
        "module snapshot",
    )?;
    let mut entry = MODULEENTRY32W {
        dwSize: u32::try_from(mem::size_of::<MODULEENTRY32W>())
            .map_err(|_error| WindowsLaunchError::new("MODULEENTRY32W size exceeds u32"))?,
        ..MODULEENTRY32W::default()
    };
    // SAFETY: snapshot is valid and entry points to initialized writable storage.
    let mut available = unsafe { Module32FirstW(snapshot.get(), &raw mut entry) } != 0;
    while available {
        let module_name = wide_slice_to_string(&entry.szModule);
        if OsStr::new(&module_name).eq_ignore_ascii_case(name) {
            return Ok(entry.modBaseAddr as usize);
        }
        // SAFETY: snapshot and entry remain valid for enumeration.
        available = unsafe { Module32NextW(snapshot.get(), &raw mut entry) } != 0;
    }
    Err(WindowsLaunchError::new(format!(
        "module {} was not loaded in process {process_id}",
        name.to_string_lossy()
    )))
}

fn create_snapshot(
    flags: u32,
    process_id: u32,
    name: &str,
) -> Result<OwnedHandle, WindowsLaunchError> {
    for attempt in 0..50_u32 {
        // SAFETY: flags and process identifier are plain values.
        let snapshot = unsafe { CreateToolhelp32Snapshot(flags, process_id) };
        if snapshot != INVALID_HANDLE_VALUE {
            return OwnedHandle::new(snapshot, name);
        }
        let error = std::io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(24) | Some(299)) && attempt < 49 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }
        return Err(WindowsLaunchError::new(format!(
            "CreateToolhelp32Snapshot({name}) failed: {error}"
        )));
    }
    Err(WindowsLaunchError::new(format!(
        "CreateToolhelp32Snapshot({name}) exhausted its retries"
    )))
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn wide_slice_to_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(value.get(..length).unwrap_or_default())
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn new(handle: HANDLE, operation: &str) -> Result<Self, WindowsLaunchError> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(WindowsLaunchError::windows(operation))
        } else {
            Ok(Self(handle))
        }
    }

    fn get(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: OwnedHandle is constructed only from owned kernel handles.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct RemoteAllocation {
    process: HANDLE,
    address: *mut c_void,
    size:    usize,
}

impl RemoteAllocation {
    fn new(process: HANDLE, size: usize) -> Result<Self, WindowsLaunchError> {
        if size == 0 {
            return Err(WindowsLaunchError::new(
                "remote allocation size cannot be zero",
            ));
        }
        // SAFETY: process is live and null requests a system-selected address.
        let address = unsafe {
            VirtualAllocEx(
                process,
                ptr::null(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if address.is_null() {
            return Err(WindowsLaunchError::windows("VirtualAllocEx"));
        }
        Ok(Self {
            process,
            address,
            size,
        })
    }

    fn get(&self) -> *mut c_void {
        self.address
    }
}

impl Drop for RemoteAllocation {
    fn drop(&mut self) {
        // SAFETY: address was allocated in process by VirtualAllocEx and
        // MEM_RELEASE requires a zero size.
        unsafe {
            VirtualFreeEx(self.process, self.address, 0, MEM_RELEASE);
        }
    }
}
