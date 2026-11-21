use crate::win::*;
use crate::*;

pub struct Store {
    pub path: Vec<u16>,
    pub kind: u32,
    pub tier: u32,
    pub found: Vec<u16>,
    pub compressed: bool,
    pub msg: Vec<u16>,
    pub err: u32,
    pub ptr_seen: bool,
}

pub static DISABLED: Mutex<Vec<(Vec<u16>, u32)>> = Mutex::new(Vec::new());

fn store_key(p: &[u16]) -> Vec<u16> {
    String::from_utf16_lossy(p).to_uppercase().encode_utf16().collect()
}

pub fn disabled_error(p: &[u16]) -> Option<u32> {
    let k = store_key(p);
    DISABLED.lock().unwrap().iter().find(|(x, _)| *x == k).map(|(_, e)| *e)
}

pub fn disable(p: &[u16], err: u32) {
    let k = store_key(p);
    let mut d = DISABLED.lock().unwrap();
    if !d.iter().any(|(x, _)| *x == k) {
        d.push((k, err));
    }
}

pub fn fname_ext(p: &[u16]) -> Vec<u16> {
    let i = p.iter().rposition(|c| *c == b'\\' as u16 || *c == b'/' as u16 || *c == b':' as u16).map(|i| i + 1).unwrap_or(0);
    p[i..].to_vec()
}

pub fn dir_of(p: &[u16]) -> Vec<u16> {
    match p.iter().rposition(|c| *c == b'\\' as u16 || *c == b'/' as u16) {
        Some(i) => p[..i].to_vec(),
        None => Vec::new(),
    }
}

pub fn sibling(p: &[u16], name: &str) -> Vec<u16> {
    let mut d = dir_of(p);
    d.push(b'\\' as u16);
    d.extend(w(name));
    d
}

pub fn compressed_name(p: &[u16]) -> Vec<u16> {
    let mut v = p.to_vec();
    if let Some(c) = v.last_mut() {
        *c = b'_' as u16;
    }
    v
}

pub fn make_dirs(path: &[u16]) -> Vec<Vec<u16>> {
    make_dirs_checked(path).0
}

pub fn make_dirs_checked(path: &[u16]) -> (Vec<Vec<u16>>, u32) {
    let d = dir_of(path);
    let mut made = Vec::new();
    let mut err = 0u32;
    let mut i = 0;
    while i <= d.len() {
        if i == d.len() || d[i] == b'\\' as u16 {
            let part = &d[..i];
            let skip = part.is_empty() || part.last() == Some(&(b':' as u16)) || part.iter().all(|c| *c == b'\\' as u16);
            if !skip && !exists(part) {
                if unsafe { CreateDirectoryW(z(part).as_ptr(), core::ptr::null()) } != 0 {
                    made.push(part.to_vec());
                    err = 0;
                } else {
                    err = unsafe { GetLastError() };
                }
            }
        }
        i += 1;
    }
    if err != 0 && exists(&d) {
        err = 0;
    }
    (made, err)
}

pub fn remove_dirs(made: &[Vec<u16>]) {
    for d in made.iter().rev() {
        unsafe { RemoveDirectoryW(z(d).as_ptr()) };
    }
}

pub fn same_file(a: &[u16], b: &[u16]) -> bool {
    unsafe {
        let mut x = FileAttr::default();
        let mut y = FileAttr::default();
        GetFileAttributesExW(z(a).as_ptr(), 0, &mut x) != 0
            && GetFileAttributesExW(z(b).as_ptr(), 0, &mut y) != 0
            && x.size_hi == y.size_hi
            && x.size_lo == y.size_lo
            && x.written == y.written
    }
}

pub fn copy_via_temp(src: &[u16], dst: &[u16], prog: &mut trace::Progress) -> bool {
    let tmp = sibling(dst, "download.error");
    unsafe {
        let mut cancel = 0i32;
        let routine = trace::copy_progress as *const () as usize;
        let mut ok = CopyFileExW(z(src).as_ptr(), z(&tmp).as_ptr(), routine, prog as *mut trace::Progress as usize, &mut cancel, 0) != 0;
        if ok {
            ok = MoveFileW(z(&tmp).as_ptr(), z(dst).as_ptr()) != 0;
            if !ok && GetLastError() == 0x20 {
                ok = same_file(&tmp, dst);
            }
        }
        let e = GetLastError();
        DeleteFileW(z(&tmp).as_ptr());
        SetLastError(e);
        ok
    }
}

pub enum Ptr {
    None,
    Path,
    Other,
}

impl Store {
    pub fn new(path: Vec<u16>) -> Store {
        let kind = classify(&path);
        Store { path, kind, tier: 0, found: Vec::new(), compressed: false, msg: Vec::new(), err: 0, ptr_seen: false }
    }

    pub fn is_file(&self) -> bool {
        self.kind == 2
    }

    pub fn tier(&mut self) -> u32 {
        if self.tier != 1 && self.tier != 2 {
            self.tier = if exists(&join(&self.path, &w("index2.txt"))) { 2 } else { 1 };
        }
        self.tier
    }

    pub fn base(&mut self, file: &[u16], supp: bool) -> Vec<u16> {
        let tier = self.tier();
        let mut v = self.path.clone();
        add_backslash(&mut v);
        if supp {
            v.extend(w("supplement\\"));
        }
        if tier == 2 {
            v.extend(file.iter().take(2));
            v.push(b'\\' as u16);
        }
        v.extend_from_slice(file);
        v.push(b'\\' as u16);
        v
    }

    pub fn layout(&mut self, file: &[u16], index: &[u16]) -> Vec<u16> {
        let mut v = self.base(file, false);
        v.extend(index.iter().map(|c| if *c == b'/' as u16 { b'\\' as u16 } else { *c }));
        add_backslash(&mut v);
        v.extend_from_slice(file);
        v
    }

    pub fn get_path(&mut self, file: &[u16], index: &[u16]) -> Vec<u16> {
        if self.is_file() {
            return self.layout(file, index);
        }
        let mut v = self.path.clone();
        if v.last() != Some(&(b'/' as u16)) {
            v.push(b'/' as u16);
        }
        v.extend_from_slice(file);
        v.push(b'/' as u16);
        v.extend(index.iter().copied());
        v.push(b'/' as u16);
        v.extend_from_slice(file);
        v
    }

    pub fn access_error(&self) -> u32 {
        let e = attr_error(&join(&self.path, &w(".")));
        if e == 2 || e == 3 {
            0
        } else {
            e
        }
    }

    pub fn disabled(&self) -> bool {
        disabled_error(&self.path).is_some()
    }

    pub fn open(&mut self) -> bool {
        if let Some(e) = disabled_error(&self.path) {
            trace::store_error(e, &self.path);
            self.err = e;
            unsafe { SetLastError(e) };
            return false;
        }
        if !self.is_file() {
            self.err = http::OFFLINE_ERROR;
            unsafe { SetLastError(http::OFFLINE_ERROR) };
            return false;
        }
        let e = self.access_error();
        if e != 0 {
            if !http::is_service() {
                disable(&self.path, e);
            }
            trace::store_error(e, &self.path);
            self.err = e;
            unsafe { SetLastError(e) };
            return false;
        }
        true
    }

    pub fn find(&mut self, file: &[u16], index: &[u16]) -> bool {
        self.compressed = false;
        self.msg.clear();
        self.ptr_seen = false;
        if !self.open() {
            return false;
        }
        let r = self.find_layout(file, index);
        self.err = if r { 0 } else { unsafe { GetLastError() } };
        r
    }

    fn find_layout(&mut self, file: &[u16], index: &[u16]) -> bool {
        self.found = self.layout(file, index);
        for _ in 0..0x20 {
            let plain = self.found.clone();
            let comp = compressed_name(&plain);
            let favor = has(SSRVOPT_FAVOR_COMPRESSED);
            let (first, second) = if favor { ((comp.clone(), true), (plain.clone(), false)) } else { ((plain.clone(), false), (comp.clone(), true)) };
            let e = attr_error(&first.0);
            if trace::path_error(self, e, &plain) {
                self.found = first.0;
                self.compressed = first.1;
                return true;
            }
            if exists(&second.0) {
                self.found = second.0;
                self.compressed = second.1;
                return true;
            }
            match self.follow_ptr(&plain) {
                Ptr::Path => continue,
                Ptr::None => {
                    if self.ptr_seen {
                        trace::print(&format!("{} not found\n", trace::ws(&self.found)));
                    }
                    unsafe { SetLastError(2) };
                    return false;
                }
                Ptr::Other => {
                    if self.ptr_seen {
                        trace::print(&format!("{} not found\n", trace::ws(&self.found)));
                    }
                    unsafe { SetLastError(11) };
                    return false;
                }
            }
        }
        unsafe { SetLastError(2) };
        false
    }

    pub fn follow_ptr(&mut self, at: &[u16]) -> Ptr {
        let ptr = sibling(at, "file.ptr");
        let Ok(data) = std::fs::read(String::from_utf16_lossy(&ptr)) else { return Ptr::None };
        self.ptr_seen = true;
        if data.is_empty() || data.len() > 0x103 {
            return Ptr::None;
        }
        let r = self.parse_ptr(&data);
        if let Ptr::Path = r {
            trace::print(&format!("{}\n", trace::ws(&ptr)));
        }
        r
    }

    pub fn parse_ptr(&mut self, data: &[u8]) -> Ptr {
        let text: Vec<u16> = if data.len() >= 2 && data[0] == 0xff && data[1] == 0xfe {
            data[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
        } else {
            let mut a = data.to_vec();
            a.push(0);
            unsafe { a2w(a.as_ptr(), a.len()) }
        };
        let text: Vec<u16> = text.iter().take_while(|c| **c != 0).copied().collect();
        let mut r = Ptr::None;
        let mut pos = 0;
        while pos < text.len() {
            trace::trace(&format!("\nSYMSRV:  The file.ptr file message is: {}\n", trace::ws(&text[pos..])));
            let end = text[pos..].iter().position(|c| *c == 10 || *c == 13).map(|i| pos + i).unwrap_or(text.len());
            let line = text[pos..end].to_vec();
            pos = end;
            while pos < text.len() && (text[pos] == 10 || text[pos] == 13) {
                pos += 1;
            }
            if line.is_empty() {
                continue;
            }
            let up = String::from_utf16_lossy(&line).to_ascii_uppercase();
            if up.starts_with("MSG:") {
                self.msg = line[4..].to_vec();
            } else if up.starts_with("PATH:") {
                self.found = line[5..].to_vec();
                r = Ptr::Path;
            } else if up.starts_with("RELPATH:") {
                self.found = join(&self.path, &line[8..]);
                r = Ptr::Path;
            } else if up.starts_with("STORE:") {
                self.found = line[6..].to_vec();
                r = Ptr::Other;
            } else if (line.len() >= 2 && line[0] == b'\\' as u16 && line[1] == b'\\' as u16) || (line.len() >= 2 && line[1] == b':' as u16) {
                self.found = line.clone();
                r = Ptr::Path;
            } else {
                self.found = join(&self.path, &line);
                r = Ptr::Path;
            }
        }
        r
    }

    pub fn copy_to(&mut self, dst: &mut Store, file: &[u16], index: &[u16], decompress: bool) -> bool {
        if !dst.is_file() {
            unsafe { SetLastError(http::OFFLINE_ERROR) };
            return false;
        }
        let mut target = dst.layout(file, index);
        if exists(&target) && !has(SSRVOPT_OVERWRITE) {
            dst.found = target;
            dst.compressed = false;
            return true;
        }
        let (made, derr) = make_dirs_checked(&target);
        if derr != 0 {
            trace::print(&format!("Can't create {}\n         {}\n", trace::ws(&dir_of(&target)), trace::error_text(derr)));
            remove_dirs(&made);
            unsafe { SetLastError(map_missing(derr)) };
            return false;
        }
        if self.compressed && !decompress {
            target = compressed_name(&target);
        }
        let ok = if self.compressed && decompress {
            let e = cab::expand(&self.found, &target);
            unsafe { SetLastError(e) };
            if e == 0 {
                trace::print(&format!("{} from {}: uncompressed\n", trace::ws(&fname_ext(&self.found)), trace::ws(&self.path)));
            }
            e == 0
        } else {
            let mut prog = trace::Progress::new(file, &self.found, &self.path);
            copy_via_temp(&self.found, &target, &mut prog)
        };
        if ok {
            dst.found = target;
            dst.compressed = self.compressed && !decompress;
            return true;
        }
        unsafe {
            let e = map_missing(GetLastError());
            remove_dirs(&made);
            SetLastError(e);
        }
        false
    }

    pub fn is_store(&self) -> bool {
        if !self.is_file() {
            unsafe { SetLastError(http::OFFLINE_ERROR) };
            return false;
        }
        exists(&join(&self.path, &w("pingme.txt")))
    }

    pub fn ensure_pingme(&self) {
        if self.is_store() {
            return;
        }
        unsafe {
            let h = CreateFileW(z(&join(&self.path, &w("pingme.txt"))).as_ptr(), 0, 0, core::ptr::null(), 1, 0x80, 0);
            if h != INVALID {
                CloseHandle(h);
            }
        }
    }

    pub fn store_file(&mut self, file: &[u16], index: &[u16], flags: u32) -> Option<Vec<u16>> {
        let name = fname_ext(file);
        let keep = flags & SYMSTOREOPT_OVERWRITE == 0;
        if self.find(&name, index) {
            unsafe { SetLastError(0x50) };
            if flags & SYMSTOREOPT_PASS_IF_EXISTS != 0 {
                return Some(self.found.clone());
            }
            if keep {
                return None;
            }
        }
        let path = self.layout(&name, index);
        let made = make_dirs(&path);
        let ok = if flags & SYMSTOREOPT_POINTER != 0 {
            write_pointer(file, &path, flags)
        } else if flags & SYMSTOREOPT_COMPRESS != 0 {
            let e = cab::compress(file, &compressed_name(&path));
            unsafe { SetLastError(e) };
            e == 0
        } else {
            unsafe { CopyFileExW(z(file).as_ptr(), z(&path).as_ptr(), 0, 0, core::ptr::null_mut(), keep as u32) != 0 }
        };
        if ok {
            self.found = path.clone();
            return Some(path);
        }
        unsafe {
            let e = GetLastError();
            remove_dirs(&made);
            SetLastError(e);
        }
        None
    }

    pub fn supplement_path(&mut self, node: &[u16], name: &[u16]) -> Vec<u16> {
        let mut v = self.base(node, true);
        v.extend_from_slice(name);
        v
    }

    pub fn get_supplement(&mut self, node: &[u16], name: &[u16]) -> bool {
        if !self.is_file() {
            unsafe { SetLastError(http::OFFLINE_ERROR) };
            return false;
        }
        let p = self.supplement_path(node, name);
        for cand in [p.clone(), compressed_name(&p)] {
            if exists(&cand) {
                self.found = cand;
                return true;
            }
        }
        unsafe { SetLastError(2) };
        false
    }

    pub fn store_supplement(&mut self, node: &[u16], file: &[u16], name: &[u16], flags: u32) -> Option<Vec<u16>> {
        let mut p = self.supplement_path(node, name);
        let made = make_dirs(&p);
        let ok = if flags & SYMSTOREOPT_COMPRESS != 0 {
            p = compressed_name(&p);
            let e = cab::compress(file, &p);
            unsafe { SetLastError(e) };
            e == 0
        } else {
            unsafe { CopyFileExW(z(file).as_ptr(), z(&p).as_ptr(), 0, 0, core::ptr::null_mut(), (flags & SYMSTOREOPT_OVERWRITE == 0) as u32) != 0 }
        };
        if ok {
            self.found = p.clone();
            return Some(p);
        }
        unsafe {
            let e = GetLastError();
            remove_dirs(&made);
            SetLastError(e);
        }
        None
    }
}

pub fn write_pointer(file: &[u16], path: &[u16], flags: u32) -> bool {
    let ptr = sibling(path, "file.ptr");
    let disp = if flags & SYMSTOREOPT_OVERWRITE != 0 { 2 } else { 1 };
    unsafe {
        let h = CreateFileW(z(&ptr).as_ptr(), GENERIC_WRITE, 0, core::ptr::null(), disp, 0x80, 0);
        if h == INVALID {
            return false;
        }
        let mut n = 0u32;
        if flags & SYMSTOREOPT_UNICODE != 0 {
            WriteFile(h, [0xffu8, 0xfe].as_ptr(), 2, &mut n, core::ptr::null_mut());
            WriteFile(h, file.as_ptr() as *const u8, (file.len() * 2) as u32, &mut n, core::ptr::null_mut());
        } else {
            let a = w2a(file);
            WriteFile(h, a.as_ptr(), a.len() as u32, &mut n, core::ptr::null_mut());
        }
        CloseHandle(h);
    }
    true
}
