use crate::wire::*;
use crate::xcrd::Xcrd;

pub const HR_NO_CONNECTION: i32 = 0x87e51001_u32 as i32;
pub const HR_PENDING: i32 = 0x800708ca_u32 as i32;
pub const HR_ACCESS_DENIED: i32 = 0x80070005_u32 as i32;
pub const HR_NOT_FOUND: i32 = 0x80070490_u32 as i32;
pub const HR_INSUFFICIENT_BUFFER: i32 = 0x8007007a_u32 as i32;
pub const HR_INVALID_ARG: i32 = 0x80070057_u32 as i32;
pub const HR_UNEXPECTED: i32 = 0x8000ffff_u32 as i32;
pub const HR_SUSPEND_TIMEOUT: i32 = 0x87e5001b_u32 as i32;

pub const LAUNCH_MODE_NEW_GAME: u32 = 1;
pub const LAUNCH_MODE_RESTART: u32 = 2;

pub const WNF_CRASH_DUMP_STATE: u64 = 0x41c60d2aa3bc4875;
pub const CAPABILITY_LOCAL_DUMPS: i32 = 0x7f;
pub const PACKAGE_CONTENT_TYPE: u32 = 0x19;
pub const PACKAGE_MOUNT_FLAGS: u32 = 1;

pub const PROGRESS_NAMES: [&str; 3] = ["Contacting Watson Server...", "Collecting Heap Dump...", "Compressing Heap Dump..."];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    FailFast { file: &'static str, line: u32 },
    Malformed(u32),
    OutOfTable(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub exe: Vec<u16>,
    pub args: Vec<u16>,
    pub mode: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    ExitPlan(Option<Launch>),
    TerminateTitle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerMessage {
    pub id: u32,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub identity: Vec<u16>,
    pub name: Vec<u16>,
    pub device_path: Vec<u16>,
    pub device: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemTime {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub milliseconds: u16,
}

pub struct GameHost {
    pub os_build: u32,
    pub local_dumps_capability: bool,
    pub crash_config: [u8; 16],
    pub install_identity: Vec<u16>,
    pub quiesce_name: Vec<u16>,
    pub dump_requested: bool,
    pub dump_clients: Vec<u64>,
    pub dumps_in_flight: i32,
    pub wnf_crash_dump_state: u32,
    pub controller: Option<Vec<ControllerMessage>>,
    pub mounts: Vec<Mount>,
    pub debug: Vec<String>,
    pub errors: Vec<(u32, i32)>,
    pub xcrd: Box<dyn Xcrd>,
    pub exe: Vec<u16>,
    pub args: Vec<u16>,
    pub title_running: bool,
    pub pending_launch: Option<LaunchNewGame>,
    pub restart: Option<LaunchRestartOnCrash>,
    pub actions: Vec<Action>,
    pub published_plan: Option<Launch>,
}

pub fn cch_printf(cch: usize, s: &[u16]) -> (Vec<u16>, i32) {
    if cch == 0 || cch - 1 > 0x7ffffffe {
        return (Vec::new(), HR_INVALID_ARG);
    }
    if s.len() <= cch - 1 {
        (s.to_vec(), 0)
    } else {
        (s[..cch - 1].to_vec(), HR_INSUFFICIENT_BUFFER)
    }
}

fn lower(c: u16) -> u16 {
    match char::from_u32(c as u32) {
        Some(ch) => {
            let mut l = ch.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) if (x as u32) < 0x10000 => x as u32 as u16,
                _ => c,
            }
        }
        None => c,
    }
}

pub fn wcsicmp_eq(a: &[u16], b: &[u16]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| lower(x) == lower(y))
}

fn cat(parts: &[&[u16]]) -> Vec<u16> {
    parts.iter().flat_map(|p| p.iter().copied()).collect()
}

impl GameHost {
    pub fn new(os_build: u32, local_dumps_capability: bool, xcrd: Box<dyn Xcrd>) -> Self {
        GameHost {
            os_build,
            local_dumps_capability,
            crash_config: [0; 16],
            install_identity: Vec::new(),
            quiesce_name: Vec::new(),
            dump_requested: false,
            dump_clients: Vec::new(),
            dumps_in_flight: 0,
            wnf_crash_dump_state: 0,
            controller: Some(Vec::new()),
            mounts: Vec::new(),
            debug: Vec::new(),
            errors: Vec::new(),
            xcrd,
            exe: Vec::new(),
            args: Vec::new(),
            title_running: false,
            pending_launch: None,
            restart: None,
            actions: Vec::new(),
            published_plan: None,
        }
    }

    pub fn set_title(&mut self, exe: &[u16], args: &[u16]) {
        self.exe = exe.to_vec();
        self.args = args.to_vec();
        self.title_running = true;
        self.dump_requested = false;
    }

    pub fn exit_plan(&self) -> Option<Launch> {
        if self.dump_requested {
            if let Some(r) = &self.restart {
                return Some(Launch { exe: self.exe.clone(), args: r.args.clone(), mode: LAUNCH_MODE_RESTART });
            }
        }
        self.pending_launch.as_ref().map(|p| Launch { exe: p.exe.clone(), args: p.args.clone(), mode: LAUNCH_MODE_NEW_GAME })
    }

    fn publish_exit_plan(&mut self) {
        let plan = self.exit_plan();
        if plan != self.published_plan {
            self.published_plan = plan.clone();
            self.actions.push(Action::ExitPlan(plan));
        }
    }

    fn terminate_title(&mut self) -> i32 {
        if !self.title_running {
            return 1;
        }
        self.title_running = false;
        self.actions.push(Action::TerminateTitle);
        0
    }

    pub fn launch_new_game(&mut self, req: &LaunchNewGame) -> i32 {
        if self.pending_launch.is_some() {
            self.errors.push((0x986, HR_UNEXPECTED));
            return HR_UNEXPECTED;
        }
        self.pending_launch = Some(req.clone());
        self.publish_exit_plan();
        let hr = self.terminate_title();
        if hr < 0 {
            self.pending_launch = None;
            self.publish_exit_plan();
        }
        hr
    }

    pub fn restart_on_crash(&mut self, req: &LaunchRestartOnCrash) -> i32 {
        self.restart = Some(req.clone());
        self.publish_exit_plan();
        0
    }

    pub fn title_exited(&mut self) -> Option<Launch> {
        self.title_running = false;
        let plan = self.exit_plan()?;
        if plan.mode == LAUNCH_MODE_RESTART {
            self.restart = None;
        } else {
            self.pending_launch = None;
        }
        self.exe = plan.exe.clone();
        self.args = plan.args.clone();
        self.dump_requested = false;
        self.title_running = true;
        self.publish_exit_plan();
        Some(plan)
    }

    pub fn launch_failed(&mut self, plan: &Launch, hr: i32) {
        self.errors.push((if plan.mode == LAUNCH_MODE_RESTART { 0x33d } else { 0x362 }, hr));
    }

    pub fn suspend_timed_out(&mut self, hr: i32, module_path: Option<&[u16]>, t: SystemTime) -> i32 {
        if hr >= 0 {
            return 0;
        }
        if self.local_dumps_capability {
            return hr;
        }
        self.record_quiesce(module_path, t)
    }

    pub fn has_progress_message(&self) -> bool {
        self.os_build >= 22621
    }

    pub fn set_launch(&mut self, install_identity: &[u16], crash_config: [u8; 16]) {
        self.install_identity = install_identity.to_vec();
        self.crash_config = crash_config;
        self.quiesce_name.clear();
    }

    pub fn record_quiesce(&mut self, module_path: Option<&[u16]>, t: SystemTime) -> i32 {
        let exe: Vec<u16> = match module_path {
            Some(p) if !p.is_empty() => {
                let p = &p[..p.len().min(0x104)];
                let start = p.iter().rposition(|&c| c == b'\\' as u16).map(|i| i + 1).unwrap_or(0);
                p[start..].to_vec()
            }
            _ => w("CurrentActiveProcess"),
        };
        let tail = format!("-{}-{}-{}-{}-{}-{}-{}", t.year, t.month, t.day, t.hour, t.minute, t.second, t.milliseconds);
        let (name, hr) = cch_printf(0x104, &cat(&[&w("Quiesce-"), &exe, &w(&tail)]));
        self.quiesce_name = name;
        if hr < 0 {
            self.errors.push((0x8eb, hr));
        }
        hr
    }

    fn notify_controller(&mut self, id: u32, payload: Vec<u8>) -> i32 {
        match self.controller.as_mut() {
            Some(q) => {
                q.push(ControllerMessage { id, payload });
                0
            }
            None => HR_NO_CONNECTION,
        }
    }

    fn set_dump_state(&mut self, on: bool, line: u32) {
        self.wnf_crash_dump_state = on as u32;
        let hr = self.notify_controller(CTL_DUMP_IN_PROGRESS, vec![on as u8]);
        if hr < 0 {
            self.errors.push((line, hr));
        }
    }

    pub fn crash_dump_file(&self) -> (Vec<u16>, i32) {
        let name = &self.quiesce_name;
        let s = if self.os_build >= 22621 {
            let root = if self.local_dumps_capability { w("D:\\LocalDumps") } else { w("s:\\temp") };
            cat(&[&root, &w("\\"), name, &w(".etl")])
        } else {
            cat(&[&w("D:\\LocalDumps\\"), name, &w(".etl")])
        };
        cch_printf(CRASH_DUMP_PATH_CCH, &s)
    }

    pub fn crash_dump_info(&mut self, client: u64) -> Message {
        self.dump_clients.push(client);
        let mut info = CrashDumpInfo { config: self.crash_config, file: Vec::new() };
        self.dump_requested = true;
        self.publish_exit_plan();
        if info.flags() & 2 != 0 {
            let (f, hr) = self.crash_dump_file();
            info.file = f;
            if hr < 0 {
                self.errors.push((if self.os_build >= 22621 { 0xd72 } else { 0xd4e }, hr));
            }
        }
        let before = self.dumps_in_flight;
        self.dumps_in_flight += 1;
        if before == 0 {
            self.debug.push("GameHostService: Crash dump initiated.\n".into());
            self.set_dump_state(true, 0xd88);
        }
        info.encode()
    }

    pub fn disconnect(&mut self, client: u64) {
        let Some(i) = self.dump_clients.iter().position(|&c| c == client) else { return };
        self.dump_clients.remove(i);
        let before = self.dumps_in_flight;
        self.dumps_in_flight -= 1;
        if before == 1 {
            self.debug.push("GameHostService: Crash dump completed.\n".into());
            self.set_dump_state(false, 0xda5);
        }
    }

    pub fn crash_dump_progress(&mut self, event: u64) -> Result<i32, Fault> {
        let name = *PROGRESS_NAMES.get(event as usize).ok_or(Fault::OutOfTable(event))?;
        self.debug.push(format!("GameHostService: Crash dump Progress -> {name}\n"));
        let hr = self.notify_controller(CTL_DUMP_PROGRESS, event.to_le_bytes().to_vec());
        if hr < 0 {
            self.errors.push((0xdc0, hr));
        }
        Ok(hr)
    }

    pub fn controller_notify(&mut self, payload: &[u8]) {
        if let Some(q) = self.controller.as_mut() {
            q.push(ControllerMessage { id: CTL_NOTIFY, payload: payload[..CONTROLLER_NOTIFY_SIZE].to_vec() });
        }
    }

    fn find_mount(&self, name: &[u16]) -> Option<usize> {
        self.mounts.iter().position(|m| wcsicmp_eq(&m.name, name))
    }

    pub fn package_mount(&mut self, req: &PackageMount) -> PackageMountResponse {
        let mut resp = PackageMountResponse { hr: 0, name: req.name.clone(), device_path: Vec::new() };
        if self.find_mount(&req.name).is_some() {
            self.errors.push((0x9b1, HR_ACCESS_DENIED));
            resp.hr = HR_ACCESS_DENIED;
            return resp;
        }
        self.mounts.push(Mount { identity: self.install_identity.clone(), name: req.name.clone(), device_path: Vec::new(), device: 0 });
        let mut adapter = 0usize;
        let mut device = 0usize;
        let hr = (|| {
            adapter = self.xcrd.open_adapter().map_err(|e| (0x9d6, e))?;
            device = self.xcrd.mount_content_type(adapter, &req.path, PACKAGE_CONTENT_TYPE, PACKAGE_MOUNT_FLAGS).map_err(|e| (0x9dd, e))?;
            let path = self.xcrd.query_device_path(device).map_err(|e| (0x9e1, e))?;
            let i = self.find_mount(&req.name).ok_or((0x9e5, HR_NOT_FOUND))?;
            self.mounts[i].device_path = path.clone();
            self.mounts[i].device = device;
            resp.device_path = path;
            Ok::<(), (u32, i32)>(())
        })();
        let keep = hr.is_ok();
        if let Err((line, e)) = hr {
            self.errors.push((line, e));
            resp.hr = e;
        }
        if !keep && device != 0 {
            let u = self.xcrd.unmount(adapter, device);
            if u < 0 {
                self.errors.push((0x9c3, u));
            }
        }
        if adapter != 0 {
            let c = self.xcrd.close_adapter(adapter);
            if c < 0 {
                self.errors.push((0x9c8, c));
            }
        }
        if !keep {
            if let Some(i) = self.find_mount(&req.name) {
                self.mounts.remove(i);
            }
        }
        resp
    }

    pub fn package_unmount(&mut self, req: &PackageUnmount) -> PackageUnmountResponse {
        let mut resp = PackageUnmountResponse { hr: 0, name: req.name.clone() };
        let Some(i) = self.find_mount(&req.name) else {
            self.errors.push((0xa0a, HR_NOT_FOUND));
            resp.hr = HR_NOT_FOUND;
            return resp;
        };
        let adapter = match self.xcrd.open_adapter() {
            Ok(a) => a,
            Err(e) => {
                self.errors.push((0x9ff, e));
                resp.hr = e;
                return resp;
            }
        };
        let hr = self.xcrd.unmount(adapter, self.mounts[i].device);
        self.mounts.remove(i);
        let c = self.xcrd.close_adapter(adapter);
        if c < 0 {
            self.errors.push((0xa04, c));
        }
        if hr < 0 {
            self.errors.push((0xa06, hr));
        }
        resp.hr = hr;
        resp
    }

    pub fn dispatch(&mut self, client: u64, msg: &Message) -> Result<Option<Message>, Fault> {
        let fail_fast = |line| Fault::FailFast { file: "clientproxy.cpp", line };
        let size_check = |used: usize| if used == msg.payload.len() { Ok(()) } else { Err(Fault::FailFast { file: "MessageQueueMessageGenerator.h", line: 0x3f6 }) };
        match msg.id {
            MSG_LAUNCH_NEW_GAME => {
                let (req, used) = LaunchNewGame::decode(&msg.payload).map_err(|_| Fault::Malformed(msg.id))?;
                size_check(used)?;
                self.launch_new_game(&req);
                Ok(None)
            }
            MSG_LAUNCH_RESTART_ON_CRASH => {
                let (req, _) = LaunchRestartOnCrash::decode(&msg.payload).map_err(|_| Fault::Malformed(msg.id))?;
                self.restart_on_crash(&req);
                Ok(None)
            }
            MSG_CRASH_DUMP_INFO => Ok(Some(self.crash_dump_info(client))),
            MSG_CONTROLLER_NOTIFY => {
                if msg.payload.len() < CONTROLLER_NOTIFY_SIZE {
                    return Err(Fault::Malformed(msg.id));
                }
                self.controller_notify(&msg.payload);
                Ok(None)
            }
            MSG_CRASH_DUMP_PROGRESS if self.has_progress_message() => {
                let b: [u8; 8] = msg.payload.get(..8).ok_or(Fault::Malformed(msg.id))?.try_into().unwrap();
                self.crash_dump_progress(u64::from_le_bytes(b))?;
                Ok(None)
            }
            MSG_PACKAGE_MOUNT => match PackageMount::decode(&msg.payload) {
                Ok((req, used)) => {
                    size_check(used)?;
                    Ok(Some(self.package_mount(&req).encode()))
                }
                Err(e) => {
                    self.errors.push((0x48, e));
                    Ok(None)
                }
            },
            MSG_PACKAGE_UNMOUNT => match PackageUnmount::decode(&msg.payload) {
                Ok((req, _)) => Ok(Some(self.package_unmount(&req).encode())),
                Err(e) => {
                    self.errors.push((0x5a, e));
                    Ok(None)
                }
            },
            _ => Err(fail_fast(0x94)),
        }
    }
}
