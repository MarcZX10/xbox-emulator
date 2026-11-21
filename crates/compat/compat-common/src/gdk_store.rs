//! Local, empty Microsoft Store (`XStore*`) for the title's local user.
//!
//! Why: once a local user exists (gdk_user), the title's privilege check runs
//! `FOnlineAsyncTaskGDKGetUserPrivilege::CheckForGDKPackageUpdate`, which calls
//! `XStoreCreateContext(user)`. Microsoft's PC runtime rejects the local user handle
//! ("Handle: does not exist") and the following `XStoreQueryGameAndDlcPackageUpdatesAsync`
//! faults inside the runtime on the null context.
//!
//! Model: a store that knows nothing. No package updates, no associated / entitled /
//! purchasable products, zero consumable balance, and purchase / redeem UI that reports
//! "cancelled". This grants the user **no** entitlements; it only lets the title's store
//! queries complete. Signatures are the public GDK ones (XStore.h).
//!
//! Specific to the same Fortnite build as gdk_user (wrappers located by symbol table).

use core::ffi::c_void;
use crate::gdk_user::{start_async, write_jump, xasync_complete, xasync_get_status, ProviderData, E_PENDING};

const S_OK: i32 = 0;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_ABORT: i32 = 0x8000_4004u32 as i32;

static CONTEXT_ANCHOR: u64 = 0x5853_544F_5245_4354; // store context handle
static QUERY_ANCHOR: u64 = 0x5853_544F_5245_5152; // empty product-query handle
fn ctx() -> usize { &CONTEXT_ANCHOR as *const u64 as usize }
fn query() -> usize { &QUERY_ANCHOR as *const u64 as usize }

fn log(func: &str, detail: &str) {
    crate::gdk_user::log(func, detail);
}

unsafe extern "system" fn ok_provider(op: u32, data: *const ProviderData) -> i32 {
    if op == 1 {
        xasync_complete((*data).async_block, S_OK, 0);
        return E_PENDING;
    }
    S_OK
}
unsafe extern "system" fn cancelled_provider(op: u32, data: *const ProviderData) -> i32 {
    if op == 1 {
        xasync_complete((*data).async_block, E_ABORT, 0);
        return E_PENDING;
    }
    S_OK
}

unsafe extern "system" fn create_context(user: usize, out: *mut usize) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    *out = ctx();
    log("XStoreCreateContext", &format!("user={user:#x} -> local empty store"));
    S_OK
}
unsafe extern "system" fn close_handle(_h: usize) {}

unsafe extern "system" fn updates_async(_ctx: usize, block: *mut c_void) -> i32 {
    log("XStoreQueryGameAndDlcPackageUpdatesAsync", "-> no updates");
    start_async(block, updates_async as usize, b"XStoreQueryGameAndDlcPackageUpdatesAsync\0", ok_provider)
}
unsafe extern "system" fn updates_count(block: *mut c_void, count: *mut u32) -> i32 {
    let hr = xasync_get_status(block);
    if hr >= 0 && !count.is_null() {
        *count = 0;
    }
    hr
}
unsafe extern "system" fn updates_result(block: *mut c_void, _count: u32, _out: *mut c_void) -> i32 {
    xasync_get_status(block)
}

unsafe extern "system" fn products_async(_ctx: usize, kinds: u32, _max: u32, block: *mut c_void) -> i32 {
    log("XStoreQuery(Associated|Entitled)ProductsAsync", &format!("kinds={kinds:#x} -> empty"));
    start_async(block, products_async as usize, b"XStoreQueryProductsAsync\0", ok_provider)
}
unsafe extern "system" fn products_by_id_async(_ctx: usize, kinds: u32, _ids: *const *const u8, n: usize, _filters: *const *const u8, _nf: usize, block: *mut c_void) -> i32 {
    log("XStoreQueryProductsAsync", &format!("kinds={kinds:#x} ids={n} -> empty"));
    start_async(block, products_async as usize, b"XStoreQueryProductsAsync\0", ok_provider)
}
unsafe extern "system" fn next_page_async(_q: usize, block: *mut c_void) -> i32 {
    start_async(block, products_async as usize, b"XStoreProductsQueryNextPageAsync\0", ok_provider)
}
unsafe extern "system" fn products_result(block: *mut c_void, out: *mut usize) -> i32 {
    let hr = xasync_get_status(block);
    if hr >= 0 && !out.is_null() {
        *out = query();
    }
    hr
}
unsafe extern "system" fn enumerate(_q: usize, _ctx: *mut c_void, _cb: *const c_void) -> i32 {
    S_OK // no products: the callback is never invoked
}
unsafe extern "system" fn has_more_pages(_q: usize) -> bool {
    false
}

unsafe extern "system" fn consumable_async(_ctx: usize, _id: *const u8, block: *mut c_void) -> i32 {
    log("XStoreQueryConsumableBalanceRemainingAsync", "-> balance 0");
    start_async(block, consumable_async as usize, b"XStoreQueryConsumableBalanceRemainingAsync\0", ok_provider)
}
unsafe extern "system" fn consumable_result(block: *mut c_void, out: *mut u32) -> i32 {
    let hr = xasync_get_status(block);
    if hr >= 0 && !out.is_null() {
        *out = 0; // XStoreConsumableResult { quantity }
    }
    hr
}

unsafe extern "system" fn purchase_ui_async(_ctx: usize, _id: *const u8, _name: *const u8, _json: *const u8, block: *mut c_void) -> i32 {
    log("XStoreShowPurchaseUIAsync", "-> cancelled (no store)");
    start_async(block, purchase_ui_async as usize, b"XStoreShowPurchaseUIAsync\0", cancelled_provider)
}
unsafe extern "system" fn redeem_ui_async(_ctx: usize, _token: *const u8, _ids: *const *const u8, _n: usize, _csv: bool, block: *mut c_void) -> i32 {
    log("XStoreShowRedeemTokenUIAsync", "-> cancelled (no store)");
    start_async(block, redeem_ui_async as usize, b"XStoreShowRedeemTokenUIAsync\0", cancelled_provider)
}
unsafe extern "system" fn ui_result(block: *mut c_void) -> i32 {
    xasync_get_status(block)
}

/// Called by gdk_user::install after the build check.
pub(crate) unsafe fn install(base: *mut u8) {
    let table: [(&str, usize, usize); 22] = [
        ("XStoreCloseContextHandle", 0x1dca7e4, close_handle as usize),
        ("XStoreCloseProductsQueryHandle", 0x1dca83c, close_handle as usize),
        ("XStoreCreateContext", 0x1dca898, create_context as usize),
        ("XStoreEnumerateProductsQuery", 0x1dca910, enumerate as usize),
        ("XStoreProductsQueryHasMorePages", 0x1dca998, has_more_pages as usize),
        ("XStoreProductsQueryNextPageAsync", 0x1dca9fc, next_page_async as usize),
        ("XStoreProductsQueryNextPageResult", 0x1dcaa70, products_result as usize),
        ("XStoreQueryAssociatedProductsAsync", 0x1dcaae4, products_async as usize),
        ("XStoreQueryAssociatedProductsResult", 0x1dcab78, products_result as usize),
        ("XStoreQueryConsumableBalanceRemainingAsync", 0x1dcabec, consumable_async as usize),
        ("XStoreQueryConsumableBalanceRemainingResult", 0x1dcac70, consumable_result as usize),
        ("XStoreQueryEntitledProductsAsync", 0x1dcace4, products_async as usize),
        ("XStoreQueryEntitledProductsResult", 0x1dcad78, products_result as usize),
        ("XStoreQueryGameAndDlcPackageUpdatesAsync", 0x1dcadec, updates_async as usize),
        ("XStoreQueryGameAndDlcPackageUpdatesResult", 0x1dcae60, updates_result as usize),
        ("XStoreQueryGameAndDlcPackageUpdatesResultCount", 0x1dcaee4, updates_count as usize),
        ("XStoreQueryProductsAsync", 0x1dcaf58, products_by_id_async as usize),
        ("XStoreQueryProductsResult", 0x1dcb014, products_result as usize),
        ("XStoreShowPurchaseUIAsync", 0x1dcb088, purchase_ui_async as usize),
        ("XStoreShowPurchaseUIResult", 0x1dcb12c, ui_result as usize),
        ("XStoreShowRedeemTokenUIAsync", 0x1dcb190, redeem_ui_async as usize),
        ("XStoreShowRedeemTokenUIResult", 0x1dcb240, ui_result as usize),
    ];
    let mut ok = 0;
    for (_, rva, target) in table {
        if write_jump(base.add(rva), target) {
            ok += 1;
        }
    }
    crate::record_raw("gdk_store", "install", &format!("local empty store installed ({ok}/22 XStore wrappers redirected)"));
}
