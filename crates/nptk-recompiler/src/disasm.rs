//! 6502 反汇编器包装

/// 反汇编一段 6502 代码
pub fn disassemble(data: &[u8], start_address: u16) -> Result<Vec<String>, String> {
    let instructions = disasm6502::from_addr_array(data, start_address)
        .map_err(|e| format!("disasm error: {}", e))?;
    Ok(instructions.iter().map(|i| format!("{}", i)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disasm_nop() {
        let result = disassemble(&[0xEA], 0x8000);
        assert!(result.is_ok());
    }
}