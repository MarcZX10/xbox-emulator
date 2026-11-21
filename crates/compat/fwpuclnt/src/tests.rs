use super::*;
use std::sync::Mutex as TestMutex;

static SERIAL: TestMutex<()> = TestMutex::new(());

fn fresh() -> std::sync::MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    *bitmaps() = [0; 4];
    g
}

fn engine() -> *mut c_void {
    0x1000 as *mut c_void
}

const OTHER: Guid = Guid { data1: 0x12345678, data2: 1, data3: 2, data4: [3; 8] };

#[test]
fn get0_null_engine_returns_fwp_e_null_pointer() {
    let _g = fresh();
    let mut ix = 7u8;
    let r = unsafe { FwpmBitmapIndexGet0(core::ptr::null_mut(), &FWPM_CONDITION_IP_LOCAL_PORT, &mut ix) };
    assert_eq!(r, 0x8032_001C);
    assert_eq!(ix, 7);
    assert_eq!(*bitmaps(), [0; 4]);
}

#[test]
fn get0_allocates_lowest_free_index_per_map() {
    let _g = fresh();
    let mut ix = 0u8;
    for want in 0..3u8 {
        assert_eq!(unsafe { FwpmBitmapIndexGet0(engine(), &FWPM_CONDITION_IP_LOCAL_ADDRESS, &mut ix) }, 0);
        assert_eq!(ix, want);
    }
    assert_eq!(unsafe { FwpmBitmapIndexGet0(engine(), &FWPM_CONDITION_IP_REMOTE_PORT, &mut ix) }, 0);
    assert_eq!(ix, 0);
    assert_eq!(unsafe { FwpmBitmapIndexFree0(engine(), &FWPM_CONDITION_IP_LOCAL_ADDRESS, &mut 1u8) }, 0);
    assert_eq!(unsafe { FwpmBitmapIndexGet0(engine(), &FWPM_CONDITION_IP_LOCAL_ADDRESS, &mut ix) }, 0);
    assert_eq!(ix, 1);
    assert_eq!(*bitmaps(), [0b111, 0, 0, 1]);
}

#[test]
fn get0_exhausted_and_unknown_map_give_ff_with_success() {
    let _g = fresh();
    let mut ix = 0u8;
    for want in 0..64u8 {
        assert_eq!(unsafe { FwpmBitmapIndexGet0(engine(), &FWPM_CONDITION_IP_REMOTE_ADDRESS, &mut ix) }, 0);
        assert_eq!(ix, want);
    }
    assert_eq!(unsafe { FwpmBitmapIndexGet0(engine(), &FWPM_CONDITION_IP_REMOTE_ADDRESS, &mut ix) }, 0);
    assert_eq!(ix, 0xFF);
    ix = 0;
    assert_eq!(unsafe { FwpmBitmapIndexGet0(engine(), &OTHER, &mut ix) }, 0);
    assert_eq!(ix, 0xFF);
    assert_eq!(*bitmaps(), [0, 0, u64::MAX, 0]);
}

#[test]
fn get0_null_ref_pointers_raise_rpc_x_null_ref_pointer() {
    let _g = fresh();
    let mut ix = 0u8;
    assert_eq!(unsafe { bitmap_index_get(engine(), core::ptr::null(), &mut ix) }, Outcome::Raise(1780));
    assert_eq!(unsafe { bitmap_index_get(engine(), &FWPM_CONDITION_IP_LOCAL_PORT, core::ptr::null_mut()) }, Outcome::Raise(1780));
    assert_eq!(unsafe { bitmap_index_get(core::ptr::null_mut(), core::ptr::null(), core::ptr::null_mut()) }, Outcome::Return(FWP_E_NULL_POINTER));
    assert_eq!(*bitmaps(), [0; 4]);
}

#[test]
fn free0_null_engine_returns_fwp_e_null_pointer() {
    let _g = fresh();
    service_get(&FWPM_CONDITION_IP_LOCAL_PORT);
    let r = unsafe { FwpmBitmapIndexFree0(core::ptr::null_mut(), &FWPM_CONDITION_IP_LOCAL_PORT, &mut 0u8) };
    assert_eq!(r, 0x8032_001C);
    assert_eq!(*bitmaps(), [0, 1, 0, 0]);
}

#[test]
fn free0_clears_bit_and_double_free_succeeds() {
    let _g = fresh();
    for _ in 0..4 {
        service_get(&FWPM_CONDITION_IP_LOCAL_PORT);
    }
    assert_eq!(unsafe { FwpmBitmapIndexFree0(engine(), &FWPM_CONDITION_IP_LOCAL_PORT, &mut 2u8) }, 0);
    assert_eq!(*bitmaps(), [0, 0b1011, 0, 0]);
    assert_eq!(unsafe { FwpmBitmapIndexFree0(engine(), &FWPM_CONDITION_IP_LOCAL_PORT, &mut 2u8) }, 0);
    assert_eq!(unsafe { FwpmBitmapIndexFree0(engine(), &FWPM_CONDITION_IP_LOCAL_PORT, &mut 63u8) }, 0);
    assert_eq!(*bitmaps(), [0, 0b1011, 0, 0]);
}

#[test]
fn free0_bad_index_or_map_raises_status_invalid_parameter() {
    let _g = fresh();
    service_get(&FWPM_CONDITION_IP_REMOTE_PORT);
    assert_eq!(unsafe { bitmap_index_free(engine(), &FWPM_CONDITION_IP_REMOTE_PORT, &0x40u8) }, Outcome::Raise(0xC000_000D));
    assert_eq!(unsafe { bitmap_index_free(engine(), &FWPM_CONDITION_IP_REMOTE_PORT, &0xFFu8) }, Outcome::Raise(0xC000_000D));
    assert_eq!(unsafe { bitmap_index_free(engine(), &OTHER, &0u8) }, Outcome::Raise(0xC000_000D));
    assert_eq!(unsafe { bitmap_index_free(engine(), core::ptr::null(), &0u8) }, Outcome::Raise(1780));
    assert_eq!(unsafe { bitmap_index_free(engine(), &FWPM_CONDITION_IP_REMOTE_PORT, core::ptr::null()) }, Outcome::Raise(1780));
    assert_eq!(*bitmaps(), [0, 0, 0, 1]);
}
