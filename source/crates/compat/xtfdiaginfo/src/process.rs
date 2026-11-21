use crate::com::*;
use crate::imports;
use crate::messaging::*;
use crate::sys::*;
use crate::titles;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, Ordering};
use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

pub struct TrackingInfo {
    pub process: HANDLE,
    pub threads: Vec<u32>,
    pub resume: AtomicBool,
}

struct Tracker(BTreeMap<u32, *mut TrackingInfo>);

unsafe impl Send for Tracker {}

static TRACKER: Mutex<Tracker> = Mutex::new(Tracker(BTreeMap::new()));

fn tracker() -> std::sync::MutexGuard<'static, Tracker> {
    TRACKER.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn track(process: HANDLE, pid: u32, threads: Vec<u32>) {
    let e = Box::into_raw(Box::new(TrackingInfo { process, threads, resume: AtomicBool::new(false) }));
    tracker().0.insert(pid, e);
}

pub fn is_tracked(pid: u32) -> bool {
    tracker().0.contains_key(&pid)
}

pub unsafe fn untrack(pid: u32) {
    let e = tracker().0.remove(&pid);
    if let Some(e) = e {
        let b = Box::from_raw(e);
        CloseHandle(b.process);
    }
}

pub fn request_resume(pid: u32) -> HRESULT {
    match tracker().0.get(&pid) {
        Some(e) => {
            unsafe { (**e).resume.store(true, Ordering::Release) };
            S_OK
        }
        None => E_PROCESS_NOT_TRACKED,
    }
}

unsafe fn resume_thread_id(tid: u32) -> HRESULT {
    let h = OpenThread(2, 1, tid);
    if !h.is_null() && ResumeThread(h) != 0 {
        CloseHandle(h);
        return S_OK;
    }
    hr_last_error()
}

unsafe fn wait_for_debugger(e: *mut TrackingInfo) -> HRESULT {
    loop {
        let mut port: usize = 0;
        let st = NtQueryInformationProcess((*e).process, 7, &mut port as *mut usize as *mut c_void, 8, null_mut());
        if st < 0 {
            return S_OK;
        }
        if port != 0 {
            for &t in (*e).threads.iter() {
                resume_thread_id(t);
            }
            let st = DbgUiIssueRemoteBreakin((*e).process);
            return if st < 0 { hr_from_nt(st) } else { S_OK };
        }
        if (*e).resume.load(Ordering::Acquire) {
            for &t in (*e).threads.iter() {
                resume_thread_id(t);
            }
            return S_OK;
        }
        Sleep(1000);
    }
}

pub unsafe extern "system" fn tracking_thread(param: *mut c_void) -> u32 {
    let pid = param as usize as u32;
    let e = match tracker().0.get(&pid) {
        Some(e) => *e,
        None => return E_PROCESS_NOT_TRACKED as u32,
    };
    let mut hr = E_PROCESS_NOT_TRACKED;
    if !e.is_null() {
        hr = wait_for_debugger(e);
    }
    untrack(pid);
    hr as u32
}

unsafe fn begin(request: usize, body: &mut Ptr, response: &mut Ptr, pid: &mut u32) -> HRESULT {
    let i = match imports::get() {
        Ok(i) => i,
        Err(e) => return e,
    };
    let hr = (i.create_json_object)(response.out());
    if hr < 0 {
        return hr;
    }
    let hr = deserialize_body(request as *mut c_void, body.out());
    if hr < 0 {
        return hr;
    }
    get_u32(body.0, &wz("Pid"), pid)
}

unsafe fn finish(response: &Ptr, result: HRESULT, reply: usize) -> HRESULT {
    set_i32(response.0, &wz("Result"), result);
    serialize_body(response.0, reply as *mut c_void)
}

pub unsafe extern "system" fn terminate_process_handler(_session: usize, request: usize, reply: usize) -> HRESULT {
    imports::log_info(b"ProcessControlTerminateProcessHandler called\0", &[]);
    let mut body = Ptr::null();
    let mut response = Ptr::null();
    let mut pid = 0u32;
    let mut res = begin(request, &mut body, &mut response, &mut pid);
    'done: {
        if res < 0 {
            break 'done;
        }
        let mut wait = false;
        if !body.is_null() {
            let mut d = 0f64;
            let hr = obj_get_number(body.0, wz("WaitOnTerminate").as_ptr(), &mut d);
            if hr != E_JSON_NOT_FOUND && hr >= 0 {
                wait = d != 0.0;
            }
        }
        let mut timeout = 0u32;
        if get_u32(body.0, &wz("WaitOnTerminateTimeout"), &mut timeout) < 0 {
            timeout = 0;
        }
        let mut code = 0u32;
        if get_u32(body.0, &wz("ExitCode"), &mut code) < 0 {
            code = 0;
        }
        let h = titles::open_process(((wait as u32) << 20) | 1, pid);
        if h.is_null() {
            res = hr_last_error();
            if res < 0 {
                break 'done;
            }
        }
        'close: {
            if TerminateProcess(h, code) == 0 {
                res = hr_last_error();
                if res < 0 {
                    break 'close;
                }
            }
            if wait {
                let r = WaitForSingleObject(h, timeout);
                res = if r == WAIT_FAILED { hr_last_error() } else { r as i32 };
            }
        }
        if !h.is_null() {
            CloseHandle(h);
        }
    }
    finish(&response, res, reply)
}

pub unsafe extern "system" fn debug_break_handler(_session: usize, request: usize, reply: usize) -> HRESULT {
    imports::log_info(b"ProcessControlDebugBreakHandler called\0", &[]);
    let mut body = Ptr::null();
    let mut response = Ptr::null();
    let mut pid = 0u32;
    let mut res = begin(request, &mut body, &mut response, &mut pid);
    let mut threads: Vec<u32> = Vec::new();
    'done: {
        if res < 0 {
            break 'done;
        }
        let h = titles::open_process(0x1F_FFFF, pid);
        if h.is_null() {
            res = hr_last_error();
            if res < 0 {
                break 'done;
            }
        }
        'close: {
            if titles::debug_active_process(pid) == 0 {
                let st = DbgUiIssueRemoteBreakin(h);
                if st < 0 {
                    res = hr_from_nt(st);
                }
                break 'close;
            }
            let mut created = false;
            let mut stop = false;
            let mut exited = false;
            let mut ev: DebugEvent = core::mem::zeroed();
            while !exited {
                if WaitForDebugEvent(&mut ev, 1000) == 0 {
                    let e = GetLastError();
                    if e == ERROR_SEM_TIMEOUT {
                        continue;
                    }
                    res = hr_from_win32(e);
                    break 'close;
                }
                let status = match ev.code {
                    1 => 0x0001_0001,
                    2 => {
                        created = true;
                        let th = ev.u[0] as HANDLE;
                        if SuspendThread(th) == 0 {
                            let e = GetLastError();
                            res = if e == 0 { E_FAIL } else { hr_from_win32(e) };
                            break 'close;
                        }
                        threads.push(GetThreadId(th));
                        0x0001_0002
                    }
                    5 | 9 => {
                        exited = true;
                        0x0001_0002
                    }
                    _ => {
                        if created {
                            stop = true;
                        }
                        0x0001_0002
                    }
                };
                if ContinueDebugEvent(ev.pid, ev.tid, status) == 0 {
                    let e = GetLastError();
                    res = if e == 0 { E_FAIL } else { hr_from_win32(e) };
                    break 'close;
                }
                if stop {
                    break;
                }
            }
            if titles::debug_active_process_stop(pid) == 0 {
                res = hr_last_error();
                if res < 0 {
                    break 'close;
                }
            }
            if exited {
                break 'close;
            }
            track(h, pid, core::mem::take(&mut threads));
            let t = CreateThread(null_mut(), 0, tracking_thread, pid as usize as *mut c_void, 0, null_mut());
            if t.is_null() {
                res = hr_last_error();
                untrack(pid);
            }
            break 'done;
        }
        if !h.is_null() {
            CloseHandle(h);
        }
    }
    finish(&response, res, reply)
}

pub unsafe extern "system" fn resume_all_threads_handler(_session: usize, request: usize, reply: usize) -> HRESULT {
    imports::log_info(b"ProcessControlResumeAllThreadsHandler called\0", &[]);
    let mut body = Ptr::null();
    let mut response = Ptr::null();
    let mut pid = 0u32;
    let mut res = begin(request, &mut body, &mut response, &mut pid);
    if res >= 0 {
        res = request_resume(pid);
    }
    finish(&response, res, reply)
}

pub unsafe extern "system" fn resume_deferred_execution_handler(_session: usize, request: usize, reply: usize) -> HRESULT {
    imports::log_info(b"ProcessControlResumeDeferredExecution called\0", &[]);
    let mut seen: HashSet<usize> = HashSet::new();
    let mut body = Ptr::null();
    let mut response = Ptr::null();
    let mut pid = 0u32;
    let mut res = begin(request, &mut body, &mut response, &mut pid);
    'done: {
        if res < 0 {
            break 'done;
        }
        let h = titles::open_process(0x400, pid);
        let mut prev: HANDLE = null_mut();
        let mut next: HANDLE = null_mut();
        'close: {
            if h.is_null() {
                let e = GetLastError();
                res = hr_from_win32(e);
                if e != 0 {
                    break 'close;
                }
            }
            loop {
                let st = NtGetNextThread(h, prev, 0x42, 0, 0, &mut next);
                if st < 0 {
                    if st != STATUS_NO_MORE_ENTRIES {
                        res = hr_from_win32(RtlNtStatusToDosError(st));
                    }
                    break 'close;
                }
                if !seen.insert(next as usize) {
                    break 'close;
                }
                if ResumeThread(next) == u32::MAX {
                    let e = GetLastError();
                    res = hr_from_win32(e);
                    if e != 0 {
                        break 'close;
                    }
                }
                CloseHandle(prev);
                prev = next;
                next = null_mut();
            }
        }
        CloseHandle(h);
        CloseHandle(next);
        CloseHandle(prev);
    }
    finish(&response, res, reply)
}
