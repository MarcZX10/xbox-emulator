use std::path::Path;

const IIDS: &[(&str, &[u8; 16])] = &[
    ("IID_ID3D12CommandAllocator", &crate::iids::IID_ID3D12CommandAllocator),
    ("IID_ID3D12CommandList", &crate::iids::IID_ID3D12CommandList),
    ("IID_ID3D12CommandQueue", &crate::iids::IID_ID3D12CommandQueue),
    ("IID_ID3D12Debug", &crate::iids::IID_ID3D12Debug),
    ("IID_ID3D12DescriptorHeap", &crate::iids::IID_ID3D12DescriptorHeap),
    ("IID_ID3D12Device", &crate::iids::IID_ID3D12Device),
    ("IID_ID3D12DeviceChild", &crate::iids::IID_ID3D12DeviceChild),
    ("IID_ID3D12Fence", &crate::iids::IID_ID3D12Fence),
    ("IID_ID3D12GraphicsCommandList", &crate::iids::IID_ID3D12GraphicsCommandList),
    ("IID_ID3D12Heap", &crate::iids::IID_ID3D12Heap),
    ("IID_ID3D12Object", &crate::iids::IID_ID3D12Object),
    ("IID_ID3D12Pageable", &crate::iids::IID_ID3D12Pageable),
    ("IID_ID3D12PipelineState", &crate::iids::IID_ID3D12PipelineState),
    ("IID_ID3D12Resource", &crate::iids::IID_ID3D12Resource),
    ("IID_ID3D12RootSignature", &crate::iids::IID_ID3D12RootSignature),
    ("IID_ID3D12RootSignatureDeserializer", &crate::iids::IID_ID3D12RootSignatureDeserializer),
    ("IID_ID3D12XboxDmaCommandList", &crate::iids::IID_ID3D12XboxDmaCommandList),
    ("IID_IDXGIAdapter", &crate::iids::IID_IDXGIAdapter),
    ("IID_IDXGIAdapter1", &crate::iids::IID_IDXGIAdapter1),
    ("IID_IDXGIAdapter2", &crate::iids::IID_IDXGIAdapter2),
    ("IID_IDXGIDevice", &crate::iids::IID_IDXGIDevice),
    ("IID_IDXGIDevice1", &crate::iids::IID_IDXGIDevice1),
    ("IID_IDXGIDevice2", &crate::iids::IID_IDXGIDevice2),
    ("IID_IDXGIDeviceSubObject", &crate::iids::IID_IDXGIDeviceSubObject),
    ("IID_IDXGIFactory", &crate::iids::IID_IDXGIFactory),
    ("IID_IDXGIFactory1", &crate::iids::IID_IDXGIFactory1),
    ("IID_IDXGIFactory2", &crate::iids::IID_IDXGIFactory2),
    ("IID_IDXGIObject", &crate::iids::IID_IDXGIObject),
    ("IID_IDXGISwapChain", &crate::iids::IID_IDXGISwapChain),
    ("IID_IDXGISwapChain1", &crate::iids::IID_IDXGISwapChain1),
    ("IID_IGraphicsUnknown", &crate::iids::IID_IGraphicsUnknown),
];

fn u16_at(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}

fn u32_at(b: &[u8], o: usize) -> usize {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize
}

fn rva_to_file(b: &[u8], rva: usize) -> usize {
    let pe = u32_at(b, 0x3c);
    let nsec = u16_at(b, pe + 6);
    let secs = pe + 24 + u16_at(b, pe + 20);
    (0..nsec)
        .find_map(|i| {
            let s = secs + i * 40;
            let va = u32_at(b, s + 12);
            let size = u32_at(b, s + 8).max(u32_at(b, s + 16));
            (rva >= va && rva < va + size).then(|| rva - va + u32_at(b, s + 20))
        })
        .expect("rva outside every section")
}

fn export_bytes(b: &[u8], name: &str) -> Option<[u8; 16]> {
    let pe = u32_at(b, 0x3c);
    let ed = rva_to_file(b, u32_at(b, pe + 24 + 112));
    let nnames = u32_at(b, ed + 24);
    let funcs = rva_to_file(b, u32_at(b, ed + 28));
    let names = rva_to_file(b, u32_at(b, ed + 32));
    let ords = rva_to_file(b, u32_at(b, ed + 36));
    for i in 0..nnames {
        let np = rva_to_file(b, u32_at(b, names + i * 4));
        let end = b[np..].iter().position(|&c| c == 0)?;
        if &b[np..np + end] == name.as_bytes() {
            let ord = u16_at(b, ords + i * 2);
            let at = rva_to_file(b, u32_at(b, funcs + ord * 4));
            return Some(b[at..at + 16].try_into().unwrap());
        }
    }
    None
}

#[test]
fn every_iid_export_matches_console_bytes() {
    let refs = xone_exports::reference_dlls("d3d12_x.dll");
    assert!(!refs.is_empty(), "no console d3d12_x.dll in the title dumps");
    for r in &refs {
        let b = std::fs::read(Path::new(r)).unwrap();
        for (name, ours) in IIDS {
            let console = export_bytes(&b, name).unwrap_or_else(|| panic!("{name} not exported by {}", r.display()));
            assert_eq!(&console, *ours, "{name} differs from {}", r.display());
        }
    }
}

#[test]
fn iid_table_covers_every_exported_iid() {
    let listed: Vec<&str> = include_str!("../exports.txt").lines().filter_map(|l| l.split_whitespace().nth(1)).filter(|n| n.starts_with("IID_")).collect();
    assert_eq!(listed.len(), IIDS.len());
    for n in listed {
        assert!(IIDS.iter().any(|(m, _)| *m == n), "{n} missing from the test table");
    }
}
