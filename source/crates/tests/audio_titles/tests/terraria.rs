use audio_titles::*;

struct Fmod {
    hal: Hal,
    ctx: ShapeContexts,
    flowgraph: ApuMem,
}

impl Fmod {
    fn init() -> Fmod {
        heap(0x4000000);
        let ctx = ShapeContexts::allocate([0x200, 0, 0, 0x400, 0x200, 0]);
        for k in [SRC, DMA, XMA] {
            assert_ne!(ctx.va(k), 0);
            assert_ne!(ctx.apu(k), 0);
        }
        let mut hal = Hal::create();
        hal.connect(0x400, 0x400);
        assert_eq!(hal.submit(2, 0, u32::MAX, &254u32.to_le_bytes(), 0), S_OK, "FMOD 0x141028b13: SubmitCommand(2, 0, -1, &254)");
        let flowgraph = ApuMem::new(0x3000, 4, 1);
        Fmod { hal, ctx, flowgraph }
    }

    fn shutdown(mut self) {
        assert_eq!(self.hal.disconnect(), S_OK, "FMOD 0x141027b94: Disconnect");
        let t = std::time::Instant::now();
        loop {
            match self.hal.pop() {
                Some(m) if msg_kind(&m) == 0x80 => break,
                Some(_) => {}
                None => {
                    assert!(t.elapsed().as_secs() < 2, "FMOD 0x141027bd5 polls PopMessage with Sleep(5) until a 0x80 DISCONNECTED message: none arrived");
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        }
        drop(self.flowgraph);
        drop(self.hal);
        drop(self.ctx);
    }
}

#[test]
fn terraria_fmod_init_and_shutdown() {
    let _g = serial();
    let f = Fmod::init();
    let n = 7u32;
    let id = 1u64;
    assert_eq!(f.hal.submit(7, id, u32::MAX, &n.to_le_bytes(), 0), S_OK, "FMOD 0x141027f2e: disable XMA ctx");
    let m = f.hal.pop().expect("COMMAND_COMPLETED for the disable");
    assert_eq!(msg_kind(&m), 0x10);
    assert_eq!(msg_id(&m), id);
    f.shutdown();
}

#[test]
fn terraria_fmod_reinit_after_shutdown() {
    let _g = serial();
    Fmod::init().shutdown();
    Fmod::init().shutdown();
}
