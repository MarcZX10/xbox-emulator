use crate::sys::*;
use crate::tlg::*;

pub fn chk(short: &[u8], long: &[u8], name: &[u8], hr: HRESULT, message: &[u8]) {
    let p = &CORE_DEV_TOOLS_TLG;
    let tags = PRIV_TAGS;
    unsafe {
        if hr < 0 && hr != E_PENDING {
            if p.enabled(0, 0) {
                let h = hr as u32;
                let d = [d_u64(&tags), d_astr(long.as_ptr()), d_astr(name.as_ptr()), d_u32(&h), d_astr(message.as_ptr())];
                p.write(&HRESULT_FAIL, core::ptr::null(), core::ptr::null(), &d);
            }
        } else if p.enabled(5, 0) {
            let d = [d_u64(&tags), d_astr(short.as_ptr()), d_astr(name.as_ptr())];
            p.write(&HRESULT_OK, core::ptr::null(), core::ptr::null(), &d);
        }
    }
}

#[macro_export]
macro_rules! xtfchk {
    ($file:literal, $func:literal, $line:literal, $name:literal, $hr:expr, $msg:literal) => {
        $crate::chk::chk(
            concat!("XtfCHK: ", $func, ": ", $line, "\0").as_bytes(),
            concat!("XtfCHK: ", $file, ": ", $func, ": ", $line, "\0").as_bytes(),
            concat!($name, "\0").as_bytes(),
            $hr,
            concat!($msg, "\0").as_bytes(),
        )
    };
}

pub struct Activity {
    pub correlation: GUID,
    pub state: u32,
    pub id: GUID,
}

impl Activity {
    pub fn start(name: &'static [u8]) -> Box<Activity> {
        let p = &CORE_DEV_TOOLS_TLG;
        let mut a = Box::new(Activity { correlation: GUID::default(), state: 0, id: GUID::default() });
        unsafe {
            UuidCreate(&mut a.correlation);
            if p.enabled(0, 0) {
                EventActivityIdControl(3, &mut a.id);
            }
            a.state = 1;
            if p.enabled(0, 0) {
                let tags = PRIV_TAGS;
                let d = [d_u64(&tags), d_guid(&a.correlation), d_astr(name.as_ptr())];
                p.write(&ACTIVITY_START, &a.id, core::ptr::null(), &d);
            }
        }
        a
    }

    pub fn stop(mut self: Box<Self>) {
        let p = &CORE_DEV_TOOLS_TLG;
        self.state = 2;
        if p.enabled(0, 0) {
            let tags = PRIV_TAGS;
            let d = [d_u64(&tags), d_guid(&self.correlation)];
            p.write(&ACTIVITY_END, &self.id, core::ptr::null(), &d);
        }
        self.state = 3;
    }
}
