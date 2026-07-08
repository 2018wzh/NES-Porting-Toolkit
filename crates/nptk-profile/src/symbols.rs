use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Symbol table mapping human-readable names to 16-bit NES addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolTable {
    pub ram: BTreeMap<String, u16>,
    pub functions: BTreeMap<String, u16>,
    pub data: BTreeMap<String, u16>,
}

/// Load a `SymbolTable` from a RON file on disk.
pub fn load_symbols(path: &str) -> Result<SymbolTable, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let table: SymbolTable = ron::from_str(&content)?;
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_symbol_table_roundtrip() {
        let table = SymbolTable {
            ram: BTreeMap::new(),
            functions: BTreeMap::new(),
            data: BTreeMap::new(),
        };
        let ron_str = ron::to_string(&table).unwrap();
        let back: SymbolTable = ron::from_str(&ron_str).unwrap();
        assert!(back.ram.is_empty());
        assert!(back.functions.is_empty());
        assert!(back.data.is_empty());
    }
}
