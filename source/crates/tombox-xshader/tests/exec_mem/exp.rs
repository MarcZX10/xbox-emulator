use super::*;

fn exp(target: u32, en: u32, done: bool, vm: bool, srcs: [u32; 4]) -> [u32; 2] {
    [0xf800_0000 | ((vm as u32) << 12) | ((done as u32) << 11) | (target << 4) | en, srcs[0] | (srcs[1] << 8) | (srcs[2] << 16) | (srcs[3] << 24)]
}

#[test]
fn exp_mrt_export() {
    let vals = [0x1111_0001u32, 0x2222_0002, 0x3333_0003, 0x4444_0004, 0x5555_0005, 0x6666_0006, 0x7777_0007, 0x8888_0008];
    let mut cs = Vec::new();
    for &(srcs, en) in &[([4u32, 5, 6, 7], 0xfu32), ([7, 5, 6, 4], 0xf), ([0, 0, 1, 1], 0xf), ([3, 2, 1, 0], 0x3), ([6, 9, 9, 9], 0x1), ([2, 3, 4, 5], 0x7)] {
        for (done, vm) in [(true, true), (true, false)] {
            let op = exp(0, en, done, vm, srcs);
            let mut p = Prog::new();
            for (k, &x) in vals.iter().enumerate() {
                p.vv(k as u32, x);
            }
            let mut words = p.w.clone();
            words.extend_from_slice(&op);
            words.push(S_ENDPGM);
            let rt: Vec<u32> = (0..4).take_while(|c| (en >> c) & 1 != 0).map(|c| vals[srcs[c] as usize]).collect();
            cs.push(MCase { m: "exp", op: op.to_vec(), words, env: Env::default(), rt, uav: vec![], tol: 0.0 });
        }
    }
    run_mem_cases("exp", &cs);
}
