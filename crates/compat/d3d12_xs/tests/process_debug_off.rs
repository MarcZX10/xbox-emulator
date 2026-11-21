mod common_process_debug;
use common_process_debug::*;

#[test]
fn instrumented_and_stock_flags_leave_pc_debug_layer_off() {
    let d = load("off");
    assert_eq!(d.set(0x0010_0000), 0);
    assert_eq!(d.get(), 0x0010_0000);
    let (hr, dev) = d.create(0x0010_0000);
    assert_eq!(hr, 0, "D3D12XboxCreateDevice");
    assert!(!dev.is_null());
    let (hr, _) = d.create(0x8000_0001);
    assert_eq!(hr, 0);
    assert_eq!(d.get(), 0x8000_0001);
    let log = d.log_text();
    assert!(log.contains("ProcessDebugFlags=0x00100000 console UMD=xgs12_i.dll (Instrumented) pc debug layer requested=false"), "{log}");
    assert!(log.contains("ProcessDebugFlags=0x80000001 console UMD=xgs12.dll (StockWithDebugFlag) pc debug layer requested=false"), "{log}");
    assert!(!log.contains("D3D12GetDebugInterface"), "{log}");
    assert!(!log.contains("debug layer ENABLED"), "{log}");
}
