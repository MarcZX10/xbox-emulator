use tombox_xshader::global_abi::TB_GLOBAL_SPACE;

pub(crate) fn refusal(needs_global: bool) -> Option<String> {
    needs_global.then(|| {
        format!(
            "shader reads or writes global memory through a 64-bit GPU VA, which d3d12_xs does not bind yet (no TbGlobal window table / window UAVs / fault record in space {TB_GLOBAL_SPACE}, and no GPU-VA-to-resource map to fill them)"
        )
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn global_memory_shader_is_refused_with_reason() {
        assert_eq!(super::refusal(false), None);
        let r = super::refusal(true).unwrap();
        assert!(r.contains("space 4000003") && r.contains("GPU VA"), "{r}");
    }
}
