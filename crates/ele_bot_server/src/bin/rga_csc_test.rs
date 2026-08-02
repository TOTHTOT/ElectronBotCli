//! RGA CSC 隔离测试 (仅 aarch64 Linux 可用).
//!
//! 背景: RK3566 上 librga 对 YUYV 输入的 CSC (YUYV->RGB) 曾两次输出全绿,
//! 根因未定 (旧系统 librga / 驱动 userptr / 调用方式). 这个测试用已知颜色
//! 的合成彩条帧隔离验证:
//!   1. 生成 8 竖条 SMPTE 彩条 YUYV 帧 (640x480)
//!   2. 分别测 CSC->RGB888 / CSC->RGBA8888 / CSC+Rot270 三条路径
//!   3. 打印每条带中心像素的实际 RGB vs 期望值 — 全绿 bug 下所有带都会
//!      变成 ~(0,135,0), 一目了然
//!
//! 用法: ./rga_csc_test

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn main() {
    use librga::{query, rect::Rect, usage::Rotation, PixelFormat, RgaBuffer, Usage};
    use std::time::Instant;

    const W: i32 = 640;
    const H: i32 = 480;

    // 8 竖条, BT.601 full-range
    let bars: [(u8, u8, u8); 8] = [
        (255, 255, 255),
        (255, 255, 0),
        (0, 255, 255),
        (0, 255, 0),
        (255, 0, 255),
        (255, 0, 0),
        (0, 0, 255),
        (0, 0, 0),
    ];

    let mut yuyv = vec![0u8; (W * H * 2) as usize];
    for y in 0..H {
        for pair in 0..(W / 2) {
            let x = pair * 2;
            let (r, g, b) = bars[(x * 8 / W) as usize];
            let ly = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8;
            let cb = ((-0.169 * r as f32 - 0.331 * g as f32 + 0.500 * b as f32) + 128.0) as u8;
            let cr = ((0.500 * r as f32 - 0.419 * g as f32 - 0.081 * b as f32) + 128.0) as u8;
            let o = (y * W * 2 + pair * 4) as usize;
            yuyv[o] = ly;
            yuyv[o + 1] = cb;
            yuyv[o + 2] = ly;
            yuyv[o + 3] = cr;
        }
    }

    println!("RGA vendor: {}", query::query_vendor().trim());
    println!("RGA version: {}", query::query_version().trim());

    // 打印未旋转输出 (竖条) 每条带中心像素
    let dump_bands = |data: &[u8], bpp: usize, label: &str| {
        println!("--- {label} ---");
        let y = (H / 2) as usize;
        for (i, &(er, eg, eb)) in bars.iter().enumerate() {
            let x = ((i * 2 + 1) * W as usize / 16).min(W as usize - 1);
            let o = (y * W as usize + x) * bpp;
            let (r, g, b) = (data[o], data[o + 1], data[o + 2]);
            let ok = (r as i16 - er as i16).abs() < 24
                && (g as i16 - eg as i16).abs() < 24
                && (b as i16 - eb as i16).abs() < 24;
            println!(
                "  band{i}: got({r:3},{g:3},{b:3}) expect({er:3},{eg:3},{eb:3}) {}",
                if ok { "OK" } else { "MISMATCH" }
            );
        }
    };

    let run_case = |dst_fmt: PixelFormat, bpp: usize, usage: Usage, label: &str| {
        let (src, _s) = RgaBuffer::from_vec(yuyv.clone(), W, H, PixelFormat::Yuyv422)
            .expect("src buffer");
        let (mut dst, dst_data) =
            RgaBuffer::from_vec_mut(vec![0u8; (W * H) as usize * bpp], W, H, dst_fmt)
                .expect("dst buffer");
        let rect = Rect::at_origin(W, H);
        let t = Instant::now();
        let r = librga::process(&src, &mut dst, None, rect, rect, None, usage);
        println!("[{label}] result={r:?} time={:?}", t.elapsed());
        if r.is_ok() && usage == Usage::empty() {
            dump_bands(&dst_data, bpp, label);
        }
    };

    run_case(PixelFormat::Rgb888, 3, Usage::empty(), "YUYV -> RGB888 (CSC only)");
    run_case(PixelFormat::Rgba8888, 4, Usage::empty(), "YUYV -> RGBA8888 (CSC only)");

    // CSC + Rot270: 输出宽高交换 (480x640), 竖条变横条, 抽查旋转方向:
    // 输入 band0 (白) 在左列, Rot270 (逆时针) 后应出现在输出底部行.
    {
        let (ow, oh) = (H, W);
        let (src, _s) = RgaBuffer::from_vec(yuyv.clone(), W, H, PixelFormat::Yuyv422)
            .expect("src buffer");
        let (mut dst, dst_data) =
            RgaBuffer::from_vec_mut(vec![0u8; (ow * oh * 3) as usize], ow, oh, PixelFormat::Rgb888)
                .expect("dst buffer");
        let t = Instant::now();
        let r = librga::process(
            &src,
            &mut dst,
            None,
            Rect::at_origin(W, H),
            Rect::at_origin(ow, oh),
            None,
            Rotation::Rot270.to_usage(),
        );
        println!("[YUYV -> RGB888 + Rot270] result={r:?} time={:?}", t.elapsed());
        if r.is_ok() {
            // 输出 (x', y') = (y, w-1-x): 输入左列 x=0 (白) -> y'=w-1 (底部行)
            let px = |x: usize, y: usize| {
                let o = (y * ow as usize + x) * 3;
                (dst_data[o], dst_data[o + 1], dst_data[o + 2])
            };
            println!(
                "  bottom-left {:?} (expect ~white), top-right {:?} (expect ~black)",
                px(10, oh as usize - 10),
                px(ow as usize - 10, 10)
            );
        }
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
fn main() {
    eprintln!("rga_csc_test 只能在 aarch64 Linux (RK3566) 上运行");
    std::process::exit(1);
}
