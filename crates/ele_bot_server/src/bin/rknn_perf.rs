//! RKNN PERF_DETAIL 诊断工具 (仅 aarch64 Linux 可用).
//!
//! 用 `RKNN_FLAG_COLLECT_PERF_MASK` 初始化模型后跑一次推理,
//! 经 `RKNN_QUERY_PERF_DETAIL` 打印逐算子耗时报告. 重点看每个算子的
//! target 是 NPU 还是 CPU — 有算子回落 CPU 是推理慢的常见根因.
//!
//! 用法: ./rknn_perf <model.rknn>

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
    use anyhow::{bail, Context};
    use rknn_sys_rs as rknn;
    use std::ffi::{c_void, CStr};
    use std::ptr::null_mut;

    let model_path = std::env::args()
        .nth(1)
        .context("用法: rknn_perf <model.rknn>")?;
    let model = std::fs::read(&model_path).context("读取模型文件失败")?;

    unsafe {
        let mut ctx: rknn::rknn_context = 0;
        let ret = rknn::rknn_init(
            &mut ctx,
            model.as_ptr() as *mut c_void,
            model.len() as u32,
            rknn::RKNN_FLAG_COLLECT_PERF_MASK,
            null_mut(),
        );
        if ret != 0 {
            bail!("rknn_init failed: {ret}");
        }

        // 查询输入张量属性, 按模型实际尺寸分配输入 (320/640 输入的模型都能跑)
        let mut attr = rknn::rknn_tensor_attr {
            index: 0,
            ..std::mem::zeroed()
        };
        let ret = rknn::rknn_query(
            ctx,
            rknn::_rknn_query_cmd_RKNN_QUERY_INPUT_ATTR,
            &mut attr as *mut _ as *mut c_void,
            std::mem::size_of::<rknn::rknn_tensor_attr>() as u32,
        );
        if ret != 0 {
            bail!("rknn_query INPUT_ATTR failed: {ret}");
        }
        println!(
            "model input: dims={:?} n_elems={} type={} fmt={}",
            &attr.dims[..attr.n_dims as usize],
            attr.n_elems,
            attr.type_,
            attr.fmt
        );

        // 喂一帧 uint8 NHWC 输入, 内容无关紧要, 只为触发逐算子计时
        let mut input_buf = vec![128u8; attr.n_elems as usize];
        let mut input = rknn::rknn_input {
            index: 0,
            buf: input_buf.as_mut_ptr() as *mut c_void,
            size: input_buf.len() as u32,
            pass_through: 0,
            type_: rknn::_rknn_tensor_type_RKNN_TENSOR_UINT8,
            fmt: rknn::_rknn_tensor_format_RKNN_TENSOR_NHWC,
        };
        let ret = rknn::rknn_inputs_set(ctx, 1, &mut input);
        if ret != 0 {
            bail!("rknn_inputs_set failed: {ret}");
        }
        let ret = rknn::rknn_run(ctx, null_mut());
        if ret != 0 {
            bail!("rknn_run failed: {ret}");
        }

        let mut detail = rknn::rknn_perf_detail {
            perf_data: null_mut(),
            data_len: 0,
        };
        let ret = rknn::rknn_query(
            ctx,
            rknn::_rknn_query_cmd_RKNN_QUERY_PERF_DETAIL,
            &mut detail as *mut _ as *mut c_void,
            std::mem::size_of::<rknn::rknn_perf_detail>() as u32,
        );
        if ret != 0 {
            bail!("rknn_query PERF_DETAIL failed: {ret}");
        }
        println!("{}", CStr::from_ptr(detail.perf_data).to_string_lossy());

        let mut run = rknn::rknn_perf_run { run_duration: 0 };
        let ret = rknn::rknn_query(
            ctx,
            rknn::_rknn_query_cmd_RKNN_QUERY_PERF_RUN,
            &mut run as *mut _ as *mut c_void,
            std::mem::size_of::<rknn::rknn_perf_run>() as u32,
        );
        if ret == 0 {
            println!("total run: {:.2} ms", run.run_duration as f64 / 1000.0);
        }

        rknn::rknn_destroy(ctx);
    }
    Ok(())
}

#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
fn main() {
    eprintln!("rknn_perf 只能在 aarch64 Linux (RK3566) 上运行");
    std::process::exit(1);
}
