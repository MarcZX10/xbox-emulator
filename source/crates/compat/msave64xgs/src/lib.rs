#![allow(non_snake_case, clippy::missing_safety_doc)]

pub mod objects;
pub mod transport;
#[cfg(test)]
mod tests;

use objects::*;
use std::ffi::c_void;

type P = *mut c_void;

#[no_mangle]
pub unsafe extern "system" fn Ordinal6958(desc: P, args: P, out: P, reply: P) -> i32 {
    create_factory(desc, args, out, reply)
}

#[no_mangle]
pub unsafe extern "system" fn Ordinal6957(holder: P, out: P) -> i32 {
    destroy_object(holder, out)
}

#[no_mangle]
pub unsafe extern "system" fn Ordinal6956(f: *mut Factory, a: P, b: P) -> i32 {
    factory_command(f, 2, a, b, None)
}

#[no_mangle]
pub unsafe extern "system" fn Ordinal6955(f: *mut Factory, a: P, b: P) -> i32 {
    factory_command(f, 3, a, b, Some((true, 0x10)))
}

#[no_mangle]
pub unsafe extern "system" fn Ordinal6954(f: *mut Factory, a: P, b: P) -> i32 {
    factory_command(f, 4, a, b, Some((true, 8)))
}

#[no_mangle]
pub unsafe extern "system" fn Ordinal6953(f: *mut Factory, a: P, b: P) -> i32 {
    create_encoder(f, a, b)
}

#[no_mangle]
pub unsafe extern "system" fn Ordinal6952(f: *mut Factory, holder: P, b: P) -> i32 {
    destroy_encoder(f, holder, b)
}

macro_rules! encoder_export {
    ($name:ident, $cmd:expr, $check:expr) => {
        #[no_mangle]
        pub unsafe extern "system" fn $name(e: *mut Encoder, a: P, b: P) -> i32 {
            encoder_command(e, $cmd, a, b, $check)
        }
    };
}

encoder_export!(Ordinal6950, 0x8, Check::B(8));
encoder_export!(Ordinal6949, 0x9, Check::B(8));
encoder_export!(Ordinal6948, 0xa, Check::B(8));
encoder_export!(Ordinal6947, 0xb, Check::B(8));
encoder_export!(Ordinal6946, 0xc, Check::A(0x10));
encoder_export!(Ordinal6944, 0xe, Check::ClosedFirst);
encoder_export!(Ordinal6943, 0xf, Check::BThenClosed(0x10));
encoder_export!(Ordinal6942, 0x10, Check::ClosedAfter);
encoder_export!(Ordinal6941, 0x11, Check::ClosedAfter);
encoder_export!(Ordinal6940, 0x12, Check::B(8));
encoder_export!(Ordinal6939, 0x13, Check::None);
encoder_export!(Ordinal6938, 0x14, Check::None);
encoder_export!(Ordinal6937, 0x15, Check::None);
encoder_export!(Ordinal6936, 0x16, Check::None);
encoder_export!(Ordinal6935, 0x17, Check::B(8));
encoder_export!(Ordinal6934, 0x22, Check::B(0x10));

#[no_mangle]
pub unsafe extern "system" fn Ordinal6945(e: *mut Encoder, a: P, b: P) -> i32 {
    encode(e, a, b)
}
